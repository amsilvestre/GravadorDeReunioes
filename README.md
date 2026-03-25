# AMS Gravador de Reuniões

Programa para gravar reuniões ou áudio do microfone e transcrever para texto usando Whisper (Local ou Cloud).

## Recursos

- 🎙️ Gravação de áudio do microfone e do sistema (loopback)
- 📝 Transcrição automática com Whisper
- ☁️ **Whisper Cloud** — Usa a API da OpenAI (requer chave de API)
- 💻 **Whisper Local** — Modelos rodando na CPU, sem necessidade de internet
- 📜 Histórico de gravações com transcrições salvas
- 🔊 Áudio gravado em 16kHz mono (otimizado para Whisper)
- 🗑️ Exclusão de gravações remove arquivos WAV e TXT

## Como Usar

### Gravação

1. Clique no botão **"Iniciar Gravação"**
2. O programa captura simultaneamente:
   - Microfone
   - Áudio do sistema (loopback)
3. Ambos são mixados em um único arquivo WAV
4. Clique em **"Parar Gravação"** para finalizar

### Transcrição

Após gravar, você pode transcrever o áudio:

1. Escolha o motor de transcrição em **Configurações**:
   - **Cloud**: Requer chave da API da OpenAI
   - **Local**: Baixa e usa modelos Whisper no PC
2. Clique em **"Transcrever"**
3. O texto será exibido na tela
4. Use **"Copiar"** ou **"Exportar"** para salvar

### Configurações

- **Motor**: Cloud (OpenAI) ou Local (CPU)
- **Modelo** (Local): tiny, base, small, medium, large
- **API Key**: Sua chave da OpenAI (apenas para Cloud)

## Instalação

### Pré-requisitos

- Windows 10/11
- [Rust](https://rustup.rs/)

### Compilação

```bash
# Clone o repositório
git clone https://github.com/amsilvestre/GravadorDeReunioes.git
cd GravadorDeReunioes

# Compile
cargo build --release

# Execute
./target/release/gravador-de-reunioes.exe
```

### Executável Pronto

O executável compilado está em:
```
target/release/gravador-de-reunioes.exe
```

## Tecnologias

- **UI**: [Slint](https://slint.dev/) (framework declarativo)
- **Áudio**: [cpal](https://crates.io/crates/cpal) (captura de áudio)
- **Transcrição**: [whisper-rs](https://github.com/tazz4843/whisper-rs) + OpenAI API
- **Banco de dados**: SQLite

## Licença

MIT
