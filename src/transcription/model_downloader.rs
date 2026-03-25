use anyhow::{Context, Result};
use futures_util::StreamExt;
use std::path::{Path, PathBuf};

/// Baixa modelo GGML do Whisper do HuggingFace se nao existir localmente.
/// Retorna o caminho do modelo (existente ou recem-baixado).
pub fn ensure_model(
    models_dir: &Path,
    model_name: &str,
    on_progress: &dyn Fn(f32),
) -> Result<PathBuf> {
    let filename = format!("ggml-{}.bin", model_name);
    let model_path = models_dir.join(&filename);

    if model_path.exists() {
        on_progress(1.0);
        return Ok(model_path);
    }

    std::fs::create_dir_all(models_dir)
        .with_context(|| format!("Falha ao criar diretorio de modelos: {:?}", models_dir))?;

    let url = format!(
        "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/{}",
        filename
    );

    eprintln!("Baixando modelo {} de {}", filename, url);

    // Usa tokio runtime para download async com progresso
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(download_file(&url, &model_path, on_progress))?;

    Ok(model_path)
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
