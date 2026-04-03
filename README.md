# AMS Gravador de Reuniões

Programa para gravar reuniões ou áudio do microfone e transcrever para texto usando Whisper (Local ou Cloud).

## Recursos

- 🎙️ Gravação de áudio do microfone e do sistema (loopback)
- 📝 Transcrição automática com Whisper
- ☁️ **Whisper Cloud** — Usa a API da OpenAI (requer chave de API)
- 💻 **Whisper Local** — Modelos rodando na CPU ou GPU (CUDA)
- 🎮 **GPU Acceleration** — Suporte a CUDA para transcrição mais rápida
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

- **Motor**: Cloud (OpenAI) ou Local
- **Modelo** (Local): tiny, base, small, medium, large
- **Hardware** (Local): CPU ou GPU (CUDA)
- **API Key**: Sua chave da OpenAI (apenas para Cloud)

## Instalação

### Pré-requisitos

- Windows 10/11
- [Rust](https://rustup.rs/)
- [NVIDIA CUDA Toolkit 13.2+](https://developer.nvidia.com/cuda-downloads) (para aceleração GPU)
- [Visual Studio 2022 ou 2026 Community](https://visualstudio.microsoft.com/) com workload "Desktop development with C++"

### Compilação

#### CPU (padrão)

```bash
cargo build --release
```

#### GPU (CUDA)

```bash
# No Windows, use o script de build:
build_cuda.bat
```

O script `build_cuda.bat` configura automaticamente:
- Variáveis de ambiente para CUDA 13.2
- Caminho para o compilador NVIDIA (nvcc)
- Flags necessárias para compatibilidade com Visual Studio

**Requisitos CUDA:**
- CUDA Toolkit 13.2 instalado em `C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v13.2`
- Placa NVIDIA com suporte a CUDA (verifique com `nvidia-smi`)

### Uso da GPU

1. Acesse **Configurações**
2. Em "Hardware", selecione **GPU (CUDA)**
3. Se a GPU for detectada, a opção estará disponível
4. Transcrições usarão a GPU para processamento mais rápido

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
