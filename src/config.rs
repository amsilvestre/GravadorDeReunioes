use crate::db::Database;
use anyhow::Result;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct AppConfig {
    /// 0 = Cloud (OpenAI), 1 = Local (Whisper)
    pub engine: i32,
    /// Indice do modelo local: 0=tiny, 1=base, 2=small, 3=medium, 4=large
    pub model_index: i32,
    /// Chave API da OpenAI
    pub api_key: String,
    /// Diretorio de saida para gravacoes
    pub output_dir: PathBuf,
    /// Diretorio dos modelos whisper
    pub models_dir: PathBuf,
}

impl AppConfig {
    pub fn load(db: &Database) -> Result<Self> {
        let engine = db
            .get_config("engine")?
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);

        let model_index = db
            .get_config("model_index")?
            .and_then(|v| v.parse().ok())
            .unwrap_or(2); // default: small

        let api_key = db.get_config("api_key")?.unwrap_or_default();

        let default_output = Self::default_output_dir()?;
        let output_dir = db
            .get_config("output_dir")?
            .map(PathBuf::from)
            .unwrap_or(default_output);

        let models_dir = Self::default_models_dir()?;

        // Garante que os diretorios existem
        std::fs::create_dir_all(&output_dir)?;
        std::fs::create_dir_all(&models_dir)?;

        Ok(Self {
            engine,
            model_index,
            api_key,
            output_dir,
            models_dir,
        })
    }

    pub fn save(&self, db: &Database) -> Result<()> {
        db.set_config("engine", &self.engine.to_string())?;
        db.set_config("model_index", &self.model_index.to_string())?;
        db.set_config("api_key", &self.api_key)?;
        db.set_config("output_dir", &self.output_dir.to_string_lossy())?;
        Ok(())
    }

    fn default_output_dir() -> Result<PathBuf> {
        let doc_dir = dirs::document_dir()
            .ok_or_else(|| anyhow::anyhow!("Nao foi possivel encontrar a pasta Documentos"))?;
        Ok(doc_dir.join("GravadorDeReunioes").join("recordings"))
    }

    fn default_models_dir() -> Result<PathBuf> {
        let config_dir = dirs::config_dir()
            .ok_or_else(|| anyhow::anyhow!("Nao foi possivel encontrar o diretorio de configuracao"))?;
        Ok(config_dir.join("GravadorDeReunioes").join("models"))
    }

    pub fn model_name(&self) -> &str {
        match self.model_index {
            0 => "tiny",
            1 => "base",
            2 => "small",
            3 => "medium",
            4 => "large",
            _ => "small",
        }
    }
}
