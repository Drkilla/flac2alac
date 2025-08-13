# FLAC to ALAC Batch Converter

A fast, parallel batch converter that transforms FLAC files to ALAC (Apple Lossless) format while preserving all metadata and cover art. Built in Rust for maximum performance and reliability.

## Features

- 🎵 **Lossless conversion** from FLAC to ALAC
- 📋 **Complete metadata preservation** (title, artist, album, genre, etc.)
- 🖼️ **Cover art preservation** 
- ⚡ **Parallel processing** for fast batch conversion
- 🔍 **Bit-perfect verification** option (PCM comparison)
- 🖥️ **Cross-platform** (Windows, macOS, Linux)
- 💻 **Dual interface**: Command line + GUI
- 📁 **Directory structure preservation**
- 🛡️ **Robust error handling**

## Installation

### Prerequisites
- **FFmpeg** must be installed and available in your system PATH
  - Windows: Download from [ffmpeg.org](https://ffmpeg.org/download.html)
  - macOS: `brew install ffmpeg`
  - Linux: `sudo apt install ffmpeg` (Ubuntu/Debian) or equivalent

### From Source
```bash
git clone https://github.com/yourusername/flac2alac-batch.git
cd flac2alac-batch
cargo build --release
```

The binary will be available at `target/release/flac2alac-batch` (Linux/macOS) or `target\release\flac2alac-batch.exe` (Windows).

## Usage

### GUI Mode (Recommended)
Launch the graphical interface:

```bash
# Linux/macOS
./target/release/flac2alac-batch --gui

# Windows
target\release\flac2alac-batch.exe --gui
```

The GUI provides:
- **Folder browsers** for input/output selection
- **Progress tracking** with real-time updates
- **Error reporting** with detailed messages
- **All conversion options** in an intuitive interface

### Command Line Mode

#### Basic usage:
```bash
# Convert single file
flac2alac-batch --input song.flac

# Convert entire directory
flac2alac-batch --input /path/to/flac/files

# Specify output directory
flac2alac-batch --input /music/flac --output /music/alac
```

#### Advanced options:
```bash
# Use 8 parallel jobs for faster conversion
flac2alac-batch --input /music --jobs 8

# Verify bit-perfect conversion (slower but ensures quality)
flac2alac-batch --input /music --verify

# Dry run (see what would be converted without doing it)
flac2alac-batch --input /music --dry-run

# Handle existing files
flac2alac-batch --input /music --overwrite replace  # replace existing
flac2alac-batch --input /music --overwrite skip     # skip existing
flac2alac-batch --input /music --overwrite prompt   # ask for each file
```

#### Full command reference:
```
Options:
  -i, --input <PATH>         FLAC file or directory containing FLAC files
  -o, --output <DIR>         Output directory (preserves structure, defaults to source directory)
  -j, --jobs <N>             Number of parallel conversion jobs
      --verify               Enable bit-perfect verification (PCM hash comparison)
      --overwrite <MODE>     How to handle existing files [default: skip] [possible values: skip, prompt, replace]
      --dry-run              Simulation mode: show what would be converted without doing it
      --gui                  Launch graphical user interface
  -h, --help                 Print help
  -V, --version              Print version
```

## How It Works

### Conversion Process
1. **File Discovery**: Recursively scans input directory for `.flac` files
2. **Parallel Processing**: Uses Rayon for CPU-efficient parallel conversion
3. **FFmpeg Integration**: Leverages FFmpeg for reliable audio conversion
4. **Metadata Mapping**: Explicitly maps FLAC Vorbis Comments to iTunes-compatible MP4 atoms
5. **Quality Verification**: Optionally compares PCM data to ensure bit-perfect conversion

### Technical Details

**Metadata Preservation**:
- Maps FLAC Vorbis Comments to MP4/iTunes tags
- Preserves cover art as attached pictures
- Maintains all standard fields (title, artist, album, date, genre, track, album_artist)
- Custom fields are preserved when possible

**Audio Quality**:
- Pure lossless conversion (FLAC → PCM → ALAC)
- No resampling or quality loss
- Optional bit-perfect verification via SHA256 hash comparison
- Preserves original bit depth and sample rate

**Performance**:
- Parallel processing using all available CPU cores
- Efficient memory usage with streaming conversion
- Progress tracking for long-running batch jobs

### FFmpeg Command Used
The tool uses this optimized FFmpeg command:
```bash
ffmpeg -i input.flac \
  -map 0:a:0 -map 0:v? \
  -c:a alac -c:v copy \
  -disposition:v attached_pic \
  -map_metadata 0 \
  -map_metadata:s:v:0 0:s:v:0 \
  output.m4a
```

## Examples

### Family Photo Organization
```bash
# Convert your entire music library
flac2alac-batch --input ~/Music/FLAC --output ~/Music/ALAC --jobs 8

# Verify quality for archival purposes
flac2alac-batch --input ~/Music/FLAC --verify --jobs 4
```

### Single Album
```bash
# Convert specific album
flac2alac-batch --input "~/Music/Artist - Album (FLAC)"

# Preview what would be converted
flac2alac-batch --input "~/Music/Artist - Album (FLAC)" --dry-run
```

## Troubleshooting

### Common Issues

**"FFmpeg not found"**:
- Ensure FFmpeg is installed and in your system PATH
- Test with `ffmpeg -version` in terminal

**"No FLAC files found"**:
- Check that input path contains `.flac` files
- Ensure you have read permissions for the directory

**Metadata missing in iTunes**:
- This is usually resolved automatically with the current version
- Try re-importing files into iTunes after conversion

**Permission errors**:
- Ensure write permissions for output directory
- On Windows, try running as administrator if needed

### Performance Tips

- Use `--jobs` parameter to match your CPU core count
- Place input/output on fast storage (SSD) for better performance
- For large libraries, consider converting in batches

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## License

This project is licensed under the MIT License - see the LICENSE file for details.

## Acknowledgments

- Built with [Rust](https://www.rust-lang.org/) for performance and safety
- Uses [FFmpeg](https://ffmpeg.org/) for audio conversion
- GUI powered by [egui](https://github.com/emilk/egui)
- Parallel processing via [Rayon](https://github.com/rayon-rs/rayon)