#![windows_subsystem = "windows"]

use slint::SharedString;
use std::io::{Read, Write};
use std::process::Command;

slint::include_modules!();

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: updater.exe <download_url>");
        return Err(anyhow::anyhow!("URL not provided"));
    }

    let download_url = &args[1];

    let app = UpdaterWindow::new()?;

    let url_clone = download_url.to_string();
    let app_handle = app.as_weak();

    std::thread::spawn(move || {
        let temp_dir = std::env::temp_dir();
        let installer_path = temp_dir.join("AMS_Gravador_Update_Setup.exe");

        let _ = app_handle.upgrade_in_event_loop(|app: UpdaterWindow| {
            app.set_status_text("Baixando atualização...".into());
        });

        let client = match reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                let _ = app_handle.upgrade_in_event_loop(move |app: UpdaterWindow| {
                    app.set_status_text(format!("Erro: {}", e).into());
                });
                return;
            }
        };

        let response = match client.get(&url_clone).send() {
            Ok(r) => r,
            Err(e) => {
                let _ = app_handle.upgrade_in_event_loop(move |app: UpdaterWindow| {
                    app.set_status_text(format!("Erro de conexão: {}", e).into());
                });
                return;
            }
        };

        if !response.status().is_success() {
            let _ = app_handle.upgrade_in_event_loop(move |app: UpdaterWindow| {
                app.set_status_text(format!("Erro HTTP: {}", response.status()).into());
            });
            return;
        }

        let total_size = response.content_length().unwrap_or(0);

        let mut file = match std::fs::File::create(&installer_path) {
            Ok(f) => f,
            Err(e) => {
                let _ = app_handle.upgrade_in_event_loop(move |app: UpdaterWindow| {
                    app.set_status_text(format!("Erro ao criar arquivo: {}", e).into());
                });
                return;
            }
        };

        let mut stream = response;
        let mut buffer = vec![0u8; 65536];
        let mut downloaded: u64 = 0;

        loop {
            let bytes_read = match stream.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) => {
                    let _ = app_handle.upgrade_in_event_loop(move |app: UpdaterWindow| {
                        app.set_status_text(format!("Erro no download: {}", e).into());
                    });
                    let _ = std::fs::remove_file(&installer_path);
                    return;
                }
            };

            if file.write_all(&buffer[..bytes_read]).is_err() {
                let _ = app_handle.upgrade_in_event_loop(|app: UpdaterWindow| {
                    app.set_status_text("Erro ao salvar arquivo".into());
                });
                let _ = std::fs::remove_file(&installer_path);
                return;
            }

            downloaded += bytes_read as u64;
            if total_size > 0 {
                let progress = downloaded as f32 / total_size as f32;
                let downloaded_mb = downloaded as f64 / (1024.0 * 1024.0);
                let total_mb = total_size as f64 / (1024.0 * 1024.0);

                let _ = app_handle.upgrade_in_event_loop(move |app: UpdaterWindow| {
                    app.set_progress(progress);
                    app.set_status_text(
                        format!("Baixando... {:.1} MB / {:.1} MB", downloaded_mb, total_mb).into(),
                    );
                });
            }
        }

        drop(stream);
        drop(buffer);
        drop(file);

        let _ = app_handle.upgrade_in_event_loop(|app: UpdaterWindow| {
            app.set_status_text("Instalando...".into());
            app.set_progress(1.0);
        });

        std::thread::sleep(std::time::Duration::from_secs(2));

        match Command::new(&installer_path).spawn() {
            Ok(_) => {}
            Err(e) => {
                let _ = app_handle.upgrade_in_event_loop(move |app: UpdaterWindow| {
                    app.set_status_text(format!("Erro ao iniciar instalador: {}", e).into());
                });
            }
        }
    });

    app.run()?;
    Ok(())
}
