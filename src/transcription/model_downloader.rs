use anyhow::{Context, Result};
use futures_util::StreamExt;
use std::path::{Path, PathBuf};

fn get_model_filenames(model_name: &str) -> Vec<String> {
    let mut filenames = vec![format!("ggml-{}.bin", model_name)];
    if model_name == "large" {
        filenames = vec![
            "ggml-large-v3-turbo.bin".to_string(),
            "ggml-large-v3.bin".to_string(),
            "ggml-large-v2.bin".to_string(),
            "ggml-large-v1.bin".to_string(),
        ];
    }
    filenames
}

/// Baixa modelo GGML do Whisper do HuggingFace se nao existir localmente.
/// Retorna o caminho do modelo (existente ou recem-baixado).
pub fn ensure_model(
    models_dir: &Path,
    model_name: &str,
    on_progress: &dyn Fn(f32),
) -> Result<PathBuf> {
    let filenames = get_model_filenames(model_name);

    for filename in &filenames {
        let model_path = models_dir.join(filename);
        if model_path.exists() {
            on_progress(1.0);
            eprintln!("Modelo encontrado: {:?}", model_path);
            return Ok(model_path);
        }
    }

    std::fs::create_dir_all(models_dir)
        .with_context(|| format!("Falha ao criar diretorio de modelos: {:?}", models_dir))?;

    for filename in &filenames {
        let model_path = models_dir.join(filename);
        let url = format!(
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/{}",
            filename
        );

        eprintln!("Baixando modelo {} de {}", filename, url);

        let rt = tokio::runtime::Runtime::new()?;
        match rt.block_on(download_file(&url, &model_path, on_progress)) {
            Ok(_) => return Ok(model_path),
            Err(e) => {
                eprintln!("Falha ao baixar {}: {}. Tentando próximo...", filename, e);
                continue;
            }
        }
    }

    anyhow::bail!(
        "Nenhuma variantedo modelo '{}' esta disponivel. Tente outro modelo (tiny, base, small, medium).",
        model_name
    )
}

async fn download_file(url: &str, dest: &Path, on_progress: &dyn Fn(f32)) -> Result<()> {
    let response = reqwest::get(url)
        .await
        .with_context(|| format!("Falha ao conectar: {}", url))?;

    if !response.status().is_success() {
        anyhow::bail!(
            "Erro ao baixar modelo: HTTP {} - Verifique o nome do modelo.",
            response.status()
        );
    }

    let total_size = response.content_length().unwrap_or(0);
    let mut downloaded: u64 = 0;

    // Arquivo temporario para evitar modelo corrompido se interrompido
    let temp_path = dest.with_extension("bin.downloading");
    let mut file = tokio::fs::File::create(&temp_path)
        .await
        .with_context(|| format!("Falha ao criar arquivo: {:?}", temp_path))?;

    let mut stream = response.bytes_stream();

    use tokio::io::AsyncWriteExt;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("Erro durante download")?;
        file.write_all(&chunk)
            .await
            .context("Erro ao escrever arquivo")?;

        downloaded += chunk.len() as u64;
        if total_size > 0 {
            on_progress(downloaded as f32 / total_size as f32);
        }
    }

    file.flush().await?;
    drop(file);

    // Renomeia para nome final (atomico no mesmo filesystem)
    std::fs::rename(&temp_path, dest)
        .with_context(|| format!("Falha ao renomear {:?} para {:?}", temp_path, dest))?;

    on_progress(1.0);
    eprintln!("Modelo baixado com sucesso: {:?}", dest);
    Ok(())
}
