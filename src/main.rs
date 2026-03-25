mod audio;
mod config;
mod db;
mod transcription;
mod ui_bridge;

slint::include_modules!();

fn main() -> anyhow::Result<()> {
    // Inicializa banco de dados SQLite
    let database = db::Database::init()?;
    let app_config = config::AppConfig::load(&database)?;

    // Cria a janela Slint
    let app = AppWindow::new()?;

    // Configura a bridge UI <-> Backend
    ui_bridge::setup(&app, database, app_config)?;

    // Carrega historico de gravacoes ao iniciar
    app.invoke_load_recordings();

    // Roda o event loop do Slint (bloqueia na main thread)
    app.run()?;

    Ok(())
}
