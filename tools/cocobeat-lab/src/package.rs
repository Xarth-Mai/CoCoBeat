use cocobeat_media::ValidatedPackage;
use std::path::Path;

pub fn build(
    audio: &Path,
    frames: u64,
    authoring_path: &Path,
    destination: &Path,
) -> Result<(), String> {
    let validated = cocobeat_media::build_authored_package(
        audio,
        frames,
        authoring_path,
        destination,
        &format!("cocobeat-lab/{}", env!("CARGO_PKG_VERSION")),
    )?;
    summary(&validated);
    println!("Committed package: {}", destination.display());
    Ok(())
}

pub fn import_authored(
    source: &Path,
    authoring_path: &Path,
    destination: &Path,
) -> Result<(), String> {
    let validated = cocobeat_media::import_authored_package(
        source,
        authoring_path,
        destination,
        &format!("cocobeat-lab/{}", env!("CARGO_PKG_VERSION")),
    )?;
    summary(&validated);
    println!("Committed package: {}", destination.display());
    Ok(())
}

pub fn import_experimental_beat(
    source: &Path,
    authoring_path: &Path,
    channel: &str,
    destination: &Path,
) -> Result<(), String> {
    let channel = match channel {
        "left" => 0,
        "right" => 1,
        _ => return Err("Explicit experimental channel must be left or right".into()),
    };
    let cancel = cocobeat_media::NativeBeatCancellation::default();
    let signal = cancel.clone();
    // This CLI owns SIGINT, including SIG_IGN inherited from a background shell
    ctrlc::set_handler(move || {
        if signal.request() {
            eprintln!("Native import cancellation requested; waiting for owned cleanup");
        } else {
            eprintln!("Native import already publishing or finished; late Ctrl+C ignored");
        }
    })
    .map_err(|error| format!("Cannot register native Ctrl+C handler: {error}"))?;
    let result = cocobeat_media::import_experimental_beat_package_with_cancellation(
        source,
        authoring_path,
        channel,
        destination,
        &format!("cocobeat-lab/{}", env!("CARGO_PKG_VERSION")),
        &cancel,
    );
    let diagnostic = cancel.diagnostics();
    if result.is_err() || !diagnostic["late_request_ns"].is_null() {
        eprintln!("Native cancellation diagnostics: {diagnostic}");
    }
    let validated = result?;
    summary(&validated);
    println!(
        "Experimental Candidate/Algorithm, confidence=None; frontend and music quality FAIL preserved"
    );
    println!(
        "Package: {}; evidence: {}",
        destination.join("package").display(),
        destination.join("evidence").display()
    );
    Ok(())
}

pub fn verify(path: &Path) -> Result<(), String> {
    let validated = cocobeat_media::validate_package(path)?;
    summary(&validated);
    Ok(())
}

fn summary(package: &ValidatedPackage) {
    let hash: String = package
        .manifest
        .package_hash
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    println!(
        "Validated package: {} frames, {} energy blocks, {} anchors, {} section cues",
        package.manifest.canonical_frames,
        package.analysis.energy.len(),
        package.chart.anchors.len(),
        package.chart.sections.len()
    );
    println!("Package BLAKE3: {hash}");
    println!("Analysis capabilities: {:?}", package.analysis.capabilities);
    println!(
        "Tempo regions: {}, repetition relations: {}",
        package.analysis.tempo_regions.len(),
        package.analysis.repetitions.len()
    );
    println!(
        "Object integrity and initial content schema verified; automatic MIR, stage compilation and encoder quality remain separate gates."
    );
}
