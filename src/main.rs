use anyhow::{anyhow, Context, Result};
use clap::{Parser, ValueEnum};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use rayon::prelude::*;
use sha2::{Digest, Sha256};
use std::ffi::OsStr;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use walkdir::WalkDir;
use std::sync::{Arc, Mutex};
use std::thread;

#[derive(Parser, Debug)]
#[command(name = "flac2alac-batch", version, about = "Conversion FLAC → ALAC (lossless) en batch, avec métadonnées et artwork")]
struct Args {
    /// Fichier .flac ou dossier contenant des .flac
    #[arg(short, long, value_name = "PATH")]
    input: Option<PathBuf>,

    /// Dossier de sortie (structure conservée). Par défaut : dossier source.
    #[arg(short, long, value_name = "DIR")]
    output: Option<PathBuf>,

    /// Parallélisme (nombre de tâches simultanées)
    #[arg(short = 'j', long, value_name = "N")]
    jobs: Option<usize>,

    /// Vérification bit-perfect (hash du PCM s32le)
    #[arg(long)]
    verify: bool,

    /// Mode d'écrasement si le fichier de sortie existe déjà
    #[arg(long, value_enum, default_value_t = OverwriteMode::Skip)]
    overwrite: OverwriteMode,

    /// Simulation : n'exécute rien, affiche les actions
    #[arg(long)]
    dry_run: bool,

    /// Lance l'interface graphique
    #[arg(long)]
    gui: bool,
}

#[derive(Copy, Clone, Eq, PartialEq, Debug, ValueEnum)]
enum OverwriteMode { Skip, Prompt, Replace }

fn main() -> Result<()> {
    let args = Args::parse();

    if args.gui {
        return run_gui();
    }

    // Mode CLI
    let input = args.input.ok_or_else(|| anyhow!("--input requis en mode CLI"))?;
    run_cli(input, args.output, args.jobs, args.verify, args.overwrite, args.dry_run)
}

fn run_gui() -> Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([800.0, 600.0]),
        ..Default::default()
    };
    
    eframe::run_native(
        "FLAC to ALAC Converter",
        options,
        Box::new(|_cc| Box::new(FlacConverterApp::default())),
    ).map_err(|e| anyhow!("Erreur GUI: {}", e))
}

fn run_cli(
    input: PathBuf,
    output: Option<PathBuf>,
    jobs: Option<usize>,
    verify: bool,
    overwrite: OverwriteMode,
    dry_run: bool,
) -> Result<()> {
    // Fixer le parallélisme si demandé
    if let Some(j) = jobs { rayon::ThreadPoolBuilder::new().num_threads(j).build_global().ok(); }

    // Vérifier ffmpeg
    ensure_ffmpeg_available()?;

    // Collecter les fichiers FLAC
    let tasks = collect_tasks(&input, output.as_deref())?;
    if tasks.is_empty() { return Err(anyhow!("Aucun fichier .flac trouvé")); }

    // UI
    let mp = MultiProgress::new();
    let style = ProgressStyle::with_template("{spinner} {msg}").unwrap();

    // Exécution en parallèle
    let results: Vec<_> = tasks
        .into_par_iter()
        .map(|(in_path, out_path)| {
            let pb = mp.add(ProgressBar::new_spinner());
            pb.set_style(style.clone());
            pb.enable_steady_tick(std::time::Duration::from_millis(80));
            pb.set_message(format!("{} → {}", in_path.display(), out_path.display()));

            let res = process_one(&in_path, &out_path, overwrite, verify, dry_run)
                .with_context(|| format!("Échec: {}", in_path.display()));

            pb.finish_and_clear();
            res
        })
        .collect();

    // Afficher erreurs éventuelles
    let mut failed = 0usize;
    for r in results {
        if let Err(e) = r { failed += 1; eprintln!("{}", e); }
    }

    if failed > 0 { Err(anyhow!("{} conversion(s) en échec", failed)) } else { Ok(()) }
}

fn ensure_ffmpeg_available() -> Result<()> {
    let out = Command::new("ffmpeg").arg("-version").stdout(Stdio::null()).stderr(Stdio::null()).status();
    match out {
        Ok(s) if s.success() => Ok(()),
        _ => Err(anyhow!("FFmpeg introuvable dans le PATH. Installe-le puis réessaie.")),
    }
}

/// Retourne (input_flac, output_m4a)
fn collect_tasks(input: &Path, out_root: Option<&Path>) -> Result<Vec<(PathBuf, PathBuf)>> {
    let mut v = Vec::new();
    if input.is_file() {
        if input.extension().and_then(OsStr::to_str).map(|e| e.eq_ignore_ascii_case("flac")).unwrap_or(false) {
            let out = default_out_path(input, out_root)?;
            v.push((input.to_path_buf(), out));
        }
    } else if input.is_dir() {
        for entry in WalkDir::new(input).follow_links(true).into_iter().filter_map(|e| e.ok()) {
            let p = entry.path();
            if p.is_file() && p.extension().and_then(OsStr::to_str).map(|e| e.eq_ignore_ascii_case("flac")).unwrap_or(false) {
                let out = map_to_out(p, input, out_root)?;
                v.push((p.to_path_buf(), out));
            }
        }
    }
    Ok(v)
}

fn default_out_path(file: &Path, out_root: Option<&Path>) -> Result<PathBuf> {
    let stem = file.file_stem().ok_or_else(|| anyhow!("Nom de fichier invalide"))?;
    let mut out = match out_root { Some(dir) => dir.join(stem), None => file.with_file_name(stem) };
    out.set_extension("m4a");
    Ok(out)
}

fn map_to_out(file: &Path, in_root: &Path, out_root: Option<&Path>) -> Result<PathBuf> {
    if let Some(out_root) = out_root {
        let rel = file.strip_prefix(in_root).unwrap_or(file);
        let mut out = out_root.join(rel);
        out.set_extension("m4a");
        Ok(out)
    } else {
        default_out_path(file, None)
    }
}

fn process_one(in_path: &Path, out_path: &Path, overwrite: OverwriteMode, verify: bool, dry_run: bool) -> Result<()> {
    // Gestion overwrite
    if out_path.exists() {
        match overwrite {
            OverwriteMode::Skip => return Ok(()),
            OverwriteMode::Prompt => {
                if !dry_run {
                    eprint!("Le fichier existe: {}. Remplacer ? [y/N] ", out_path.display());
                    std::io::stderr().flush().ok();
                    let mut buf = String::new();
                    std::io::stdin().read_line(&mut buf).ok();
                    let yes = matches!(buf.trim(), "y" | "Y" | "o" | "O" | "oui" | "Oui");
                    if !yes { return Ok(()); }
                }
            }
            OverwriteMode::Replace => { /* continue */ }
        }
    }

    if dry_run {
        println!("[DRY-RUN] {} -> {}", in_path.display(), out_path.display());
        return Ok(());
    }

    // Créer dossier destination
    if let Some(parent) = out_path.parent() { fs::create_dir_all(parent)?; }

    // Conversion lossless + métadonnées + artwork
    run_ffmpeg_convert(in_path, out_path)?;

    if verify {
        let ok = verify_bitperfect(in_path, out_path)?;
        if !ok {
            return Err(anyhow!("Vérification bit-perfect échouée pour {}", out_path.display()));
        }
    }

    Ok(())
}

fn run_ffmpeg_convert(input: &Path, output: &Path) -> Result<()> {
    let status = Command::new("ffmpeg")
        .args([
         "-hide_banner",
         "-v", "warning",
         "-y",
         "-i", input.to_string_lossy().as_ref(),
         "-map", "0:a:0",
         "-map", "0:v?",
         "-c:a", "alac",
         "-c:v", "copy",
         "-disposition:v", "attached_pic",
         "-map_metadata", "0",
      output.to_string_lossy().as_ref(),
        ])
        .status()
        .with_context(|| "Impossible d'exécuter ffmpeg")?;

    if !status.success() {
        return Err(anyhow!("ffmpeg a échoué pour {}", input.display()));
    }
    Ok(())
}


fn pcm_sha256_from(input: &Path) -> Result<Vec<u8>> {
    let mut hasher = Sha256::new();
    let mut child = Command::new("ffmpeg")
        .args([
            "-hide_banner", "-v", "error",
            "-i", input.to_string_lossy().as_ref(),
            // PCM brut 32 bits LE : supporte 16/24 bits sans perte
            "-f", "s32le",
            "-acodec", "pcm_s32le",
            // Laisser le layout et le nombre de canaux d'origine
            "pipe:1",
        ])
        .stdout(Stdio::piped())
        .spawn()
        .with_context(|| "Échec du lancement ffmpeg pour vérification")?;

    if let Some(mut stdout) = child.stdout.take() {
        let mut buf = [0u8; 64 * 1024];
        loop {
            let n = stdout.read(&mut buf)?;
            if n == 0 { break; }
            hasher.update(&buf[..n]);
        }
    }

    let status = child.wait()?;
    if !status.success() {
        return Err(anyhow!("ffmpeg a échoué lors du décodage PCM de {}", input.display()));
    }

    Ok(hasher.finalize().to_vec())
}

fn verify_bitperfect(flac: &Path, alac: &Path) -> Result<bool> {
    let h1 = pcm_sha256_from(flac)?;
    let h2 = pcm_sha256_from(alac)?;
    Ok(h1 == h2)
}

struct FlacConverterApp {
    input_folder: String,
    output_folder: String,
    jobs: usize,
    verify: bool,
    overwrite_mode: OverwriteMode,
    dry_run: bool,
    conversion_status: Arc<Mutex<ConversionStatus>>,
    is_converting: bool,
}

impl Default for FlacConverterApp {
    fn default() -> Self {
        Self {
            input_folder: String::new(),
            output_folder: String::new(),
            jobs: 4,
            verify: false,
            overwrite_mode: OverwriteMode::Skip,
            dry_run: false,
            conversion_status: Arc::new(Mutex::new(ConversionStatus::default())),
            is_converting: false,
        }
    }
}

#[derive(Default)]
struct ConversionStatus {
    total_files: usize,
    completed_files: usize,
    current_file: String,
    errors: Vec<String>,
    is_done: bool,
}

impl eframe::App for FlacConverterApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("FLAC to ALAC Converter");
            ui.separator();

            // Input folder selection
            ui.horizontal(|ui| {
                ui.label("Input folder:");
                ui.text_edit_singleline(&mut self.input_folder);
                if ui.button("Browse...").clicked() {
                    if let Some(path) = rfd::FileDialog::new().pick_folder() {
                        self.input_folder = path.display().to_string();
                    }
                }
            });

            // Output folder selection
            ui.horizontal(|ui| {
                ui.label("Output folder:");
                ui.text_edit_singleline(&mut self.output_folder);
                if ui.button("Browse...").clicked() {
                    if let Some(path) = rfd::FileDialog::new().pick_folder() {
                        self.output_folder = path.display().to_string();
                    }
                }
            });

            ui.separator();

            // Options
            ui.horizontal(|ui| {
                ui.label("Parallel jobs:");
                ui.add(egui::Slider::new(&mut self.jobs, 1..=16));
            });

            ui.checkbox(&mut self.verify, "Verify bit-perfect conversion");
            ui.checkbox(&mut self.dry_run, "Dry run (simulation only)");

            ui.horizontal(|ui| {
                ui.label("Overwrite mode:");
                egui::ComboBox::from_label("")
                    .selected_text(format!("{:?}", self.overwrite_mode))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.overwrite_mode, OverwriteMode::Skip, "Skip");
                        ui.selectable_value(&mut self.overwrite_mode, OverwriteMode::Prompt, "Prompt");
                        ui.selectable_value(&mut self.overwrite_mode, OverwriteMode::Replace, "Replace");
                    });
            });

            ui.separator();

            // Conversion controls
            ui.horizontal(|ui| {
                if ui.button("Convert").clicked() && !self.is_converting && !self.input_folder.is_empty() {
                    self.start_conversion();
                }

                if self.is_converting {
                    ui.label("Converting...");
                }
            });

            // Progress display
            let status = self.conversion_status.lock().unwrap();
            if status.total_files > 0 {
                let progress = status.completed_files as f32 / status.total_files as f32;
                ui.add(egui::ProgressBar::new(progress).text(format!("{}/{}", status.completed_files, status.total_files)));
                
                if !status.current_file.is_empty() {
                    ui.label(format!("Current: {}", status.current_file));
                }

                // Show errors
                if !status.errors.is_empty() {
                    ui.separator();
                    ui.label("Errors:");
                    for error in &status.errors {
                        ui.colored_label(egui::Color32::RED, error);
                    }
                }

                if status.is_done {
                    if status.errors.is_empty() {
                        ui.colored_label(egui::Color32::GREEN, "Conversion completed successfully!");
                    } else {
                        ui.colored_label(egui::Color32::YELLOW, "Conversion completed with errors.");
                    }
                    self.is_converting = false;
                }
            }
        });

        if self.is_converting {
            ctx.request_repaint();
        }
    }
}

impl FlacConverterApp {
    fn start_conversion(&mut self) {
        self.is_converting = true;
        let status = Arc::clone(&self.conversion_status);
        
        // Reset status
        {
            let mut s = status.lock().unwrap();
            *s = ConversionStatus::default();
        }

        let input_folder = self.input_folder.clone();
        let output_folder = if self.output_folder.is_empty() { None } else { Some(self.output_folder.clone()) };
        let jobs = self.jobs;
        let verify = self.verify;
        let overwrite = self.overwrite_mode;
        let dry_run = self.dry_run;

        thread::spawn(move || {
            if let Err(e) = Self::run_conversion_thread(input_folder, output_folder, jobs, verify, overwrite, dry_run, status.clone()) {
                let mut s = status.lock().unwrap();
                s.errors.push(format!("Conversion failed: {}", e));
                s.is_done = true;
            }
        });
    }

    fn run_conversion_thread(
        input_folder: String,
        output_folder: Option<String>,
        jobs: usize,
        verify: bool,
        overwrite: OverwriteMode,
        dry_run: bool,
        status: Arc<Mutex<ConversionStatus>>,
    ) -> Result<()> {
        if let Some(j) = Some(jobs) { rayon::ThreadPoolBuilder::new().num_threads(j).build_global().ok(); }

        ensure_ffmpeg_available()?;

        let input_path = PathBuf::from(input_folder);
        let output_path = output_folder.map(PathBuf::from);
        let tasks = collect_tasks(&input_path, output_path.as_deref())?;
        
        if tasks.is_empty() {
            return Err(anyhow!("No FLAC files found"));
        }

        // Update total files
        {
            let mut s = status.lock().unwrap();
            s.total_files = tasks.len();
        }

        let status_for_thread = Arc::clone(&status);
        let results: Vec<_> = tasks
            .into_par_iter()
            .map(|(in_path, out_path)| {
                // Update current file
                {
                    let mut s = status_for_thread.lock().unwrap();
                    s.current_file = format!("{} → {}", in_path.file_name().unwrap_or_default().to_string_lossy(), out_path.file_name().unwrap_or_default().to_string_lossy());
                }

                let res = process_one(&in_path, &out_path, overwrite, verify, dry_run);

                // Update progress
                {
                    let mut s = status_for_thread.lock().unwrap();
                    s.completed_files += 1;
                    if let Err(e) = &res {
                        s.errors.push(format!("{}: {}", in_path.display(), e));
                    }
                }

                res
            })
            .collect();

        // Mark as done
        {
            let mut s = status.lock().unwrap();
            s.is_done = true;
            s.current_file.clear();
        }

        Ok(())
    }
}
