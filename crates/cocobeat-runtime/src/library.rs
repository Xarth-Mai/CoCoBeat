//! Directory names are candidates, never validated SongPackage metadata
use crate::content::{self, SongContent};
use kira::sound::static_sound::StaticSoundData;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
};

pub(crate) fn default_root() -> Result<PathBuf, String> {
    let absolute = |name| {
        std::env::var_os(name)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
    };
    #[cfg(windows)]
    let root = absolute("LOCALAPPDATA")
        .ok_or("LOCALAPPDATA must contain an absolute data directory")?
        .join("CoCoBeat/Songs");
    #[cfg(not(windows))]
    let root = absolute("XDG_DATA_HOME")
        .or_else(|| absolute("HOME").map(|home| home.join(".local/share")))
        .ok_or("XDG_DATA_HOME or HOME must contain an absolute data directory")?
        .join("cocobeat/songs");
    Ok(root)
}

const MAX_DIRECTORY_ENTRIES: usize = 512;
const MAX_CANDIDATES: usize = 128;
static NEXT_SOURCE_IMPORT: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Candidate {
    pub path: PathBuf,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Discovery<T = Candidate> {
    pub missing: bool,
    pub candidates: Vec<T>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SourceCandidate {
    pub source: PathBuf,
    pub authoring: PathBuf,
}

pub(crate) struct LoadedSong {
    pub path: Option<PathBuf>,
    pub song_id: Option<String>,
    pub content: SongContent,
    pub sound: StaticSoundData,
}

pub(crate) enum Update {
    Discovered(Discovery),
    Sources(Discovery<SourceCandidate>),
    Loaded(Box<LoadedSong>),
}

pub(crate) struct Library {
    root: PathBuf,
    worker: Option<JoinHandle<()>>,
    importing: bool,
    result: Option<mpsc::Receiver<Result<Update, String>>>,
}

impl Library {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            worker: None,
            importing: false,
            result: None,
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn busy(&self) -> bool {
        self.worker.is_some()
    }

    pub fn importing(&self) -> bool {
        self.importing && self.busy()
    }

    pub fn is_finished(&self) -> bool {
        self.worker.as_ref().is_none_or(JoinHandle::is_finished)
    }

    pub fn scan(&mut self) -> Result<(), String> {
        let root = self.root.clone();
        self.spawn(move || discover(&root).map(Update::Discovered))
    }

    pub fn scan_sources(&mut self) -> Result<(), String> {
        let root = self.root.join("imports");
        self.spawn(move || discover_sources(&root).map(Update::Sources))
    }

    pub fn new_import_destination(&self) -> Result<PathBuf, String> {
        if !fs::symlink_metadata(&self.root)
            .map_err(|error| error.to_string())?
            .is_dir()
        {
            return Err("Song library must be a real directory".into());
        }
        for _ in 0..32 {
            let number = NEXT_SOURCE_IMPORT.fetch_add(1, Ordering::Relaxed);
            let destination = self
                .root
                .join(format!("imported-{}-{number}", std::process::id()));
            match fs::symlink_metadata(&destination) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    return Ok(destination);
                }
                Ok(_) => {}
                Err(error) => return Err(format!("Inspect import destination: {error}")),
            }
        }
        Err("Cannot select a new import destination after 32 attempts".into())
    }

    pub fn import_authored(
        &mut self,
        source: SourceCandidate,
        destination: PathBuf,
    ) -> Result<(), String> {
        if destination.parent() != Some(self.root.as_path()) {
            return Err("Import destination must be a new directory in the song library".into());
        }
        self.spawn(move || {
            cocobeat_media::import_authored_package(
                &source.source,
                &source.authoring,
                &destination,
                &format!("cocobeat-game/{}", env!("CARGO_PKG_VERSION")),
            )?;
            load_song(Some(destination.clone())).map_err(|error| {
                format!(
                    "Imported package saved at {}; runtime loading failed: {error}",
                    destination.display()
                )
            })
        })?;
        self.importing = true;
        Ok(())
    }

    pub fn load(&mut self, path: Option<PathBuf>) -> Result<(), String> {
        self.spawn(move || load_song(path))
    }

    fn spawn(
        &mut self,
        job: impl FnOnce() -> Result<Update, String> + Send + 'static,
    ) -> Result<(), String> {
        if self.busy() {
            return Err("The previous library worker must finish and be polled first".into());
        }
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("cocobeat-library".into())
            .spawn(move || {
                let _ = sender.send(job());
            })
            .map_err(|error| format!("Start library worker: {error}"))?;
        self.importing = false;
        self.worker = Some(worker);
        self.result = Some(receiver);
        Ok(())
    }

    /// Discards a pending result while retaining ownership of the running worker
    pub fn cancel(&mut self) {
        self.result = None;
    }

    /// Results become visible only after the single worker has actually exited
    pub fn poll(&mut self) -> Result<Option<Update>, String> {
        if !self.is_finished() {
            return Ok(None);
        }
        if let Some(worker) = self.worker.take()
            && worker.join().is_err()
        {
            self.result = None;
            return Err("Library worker panicked".into());
        }
        self.importing = false;
        let Some(receiver) = self.result.take() else {
            return Ok(None);
        };
        match receiver.try_recv() {
            Ok(result) => result.map(Some),
            Err(error) => Err(format!("Library worker ended without a result: {error}")),
        }
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        self.cancel();
        if let Some(worker) = self.worker.take() {
            // Closing normally polls completion first; teardown must never detach owned work
            let _ = worker.join();
        }
    }
}

fn load_song(path: Option<PathBuf>) -> Result<Update, String> {
    let (content, sound, song_id) = match &path {
        Some(path) => {
            let (content, sound, id) =
                content::load_package_named(path, cocobeat_stage::COMPILER_VERSION)?;
            (content, sound, Some(id))
        }
        None => (
            SongContent::development(),
            content::development_sound(),
            None,
        ),
    };
    Ok(Update::Loaded(Box::new(LoadedSong {
        path,
        song_id,
        content,
        sound,
    })))
}

fn discover_sources(root: &Path) -> Result<Discovery<SourceCandidate>, String> {
    let metadata = match fs::symlink_metadata(root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Discovery {
                missing: true,
                candidates: vec![],
            });
        }
        Err(error) => return Err(format!("Inspect import folder: {error}")),
    };
    if !metadata.is_dir() {
        return Err("Import folder must be a directory, not a symlink".into());
    }
    let mut candidates = Vec::new();
    for (index, entry) in fs::read_dir(root)
        .map_err(|error| format!("Read import folder: {error}"))?
        .enumerate()
    {
        if index >= MAX_DIRECTORY_ENTRIES {
            return Err("Import folder exceeds 512 immediate entries".into());
        }
        let entry = entry.map_err(|error| format!("Read import entry: {error}"))?;
        if !entry
            .file_type()
            .map_err(|error| error.to_string())?
            .is_file()
        {
            continue;
        }
        let source = entry.path();
        let Some(extension) = source.extension().and_then(|value| value.to_str()) else {
            continue;
        };
        if !["wav", "flac", "mp3", "ogg"]
            .iter()
            .any(|allowed| extension.eq_ignore_ascii_case(allowed))
        {
            continue;
        }
        let authoring = source.with_extension("authoring.json");
        match fs::symlink_metadata(&authoring) {
            Ok(metadata) if metadata.is_file() => {
                if candidates.len() == MAX_CANDIDATES {
                    return Err("Import folder exceeds 128 paired candidates".into());
                }
                candidates.push(SourceCandidate { source, authoring });
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("Inspect paired authoring: {error}")),
        }
    }
    candidates.sort_by(|a, b| a.source.file_name().cmp(&b.source.file_name()));
    Ok(Discovery {
        missing: false,
        candidates,
    })
}

fn discover(root: &Path) -> Result<Discovery, String> {
    let metadata = match fs::symlink_metadata(root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Discovery {
                missing: true,
                candidates: vec![],
            });
        }
        Err(error) => return Err(format!("Inspect song library: {error}")),
    };
    if !metadata.is_dir() {
        return Err("Song library must be a directory, not a symlink".into());
    }
    let mut candidates = Vec::new();
    for (index, entry) in fs::read_dir(root)
        .map_err(|error| format!("Read song library: {error}"))?
        .enumerate()
    {
        if index >= MAX_DIRECTORY_ENTRIES {
            return Err("Song library exceeds 512 immediate entries".into());
        }
        let entry = entry.map_err(|error| format!("Read song library entry: {error}"))?;
        let kind = entry
            .file_type()
            .map_err(|error| format!("Inspect song library entry: {error}"))?;
        if !kind.is_dir() {
            continue;
        }
        let path = entry.path();
        match fs::symlink_metadata(path.join("song.package")) {
            Ok(metadata) if metadata.is_file() => {
                if candidates.len() == MAX_CANDIDATES {
                    return Err("Song library exceeds 128 package candidates".into());
                }
                candidates.push(Candidate { path });
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("Inspect package candidate: {error}")),
        }
    }
    candidates.sort_by(|a, b| a.path.file_name().cmp(&b.path.file_name()));
    Ok(Discovery {
        missing: false,
        candidates,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drain(library: &mut Library) -> Result<Option<Update>, String> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !library.is_finished() {
            assert!(
                std::time::Instant::now() < deadline,
                "Library worker did not finish"
            );
            thread::yield_now();
        }
        library.poll()
    }

    #[test]
    fn cancellation_keeps_worker_owned_and_old_results_cannot_cross_into_new_jobs() {
        let mut library = Library::new(PathBuf::from("unused-library-root"));
        assert_eq!(library.root(), Path::new("unused-library-root"));
        assert!(!library.busy());
        let (release, wait) = mpsc::sync_channel(1);
        library
            .spawn(move || {
                wait.recv().unwrap();
                Ok(Update::Discovered(Discovery {
                    missing: false,
                    candidates: vec![],
                }))
            })
            .unwrap();
        library.cancel();
        assert!(library.busy());
        assert!(!library.is_finished());
        assert!(library.poll().unwrap().is_none());
        assert!(library.scan().is_err());
        release.send(()).unwrap();
        assert!(drain(&mut library).unwrap().is_none());
        assert!(!library.busy());
        library
            .spawn(|| Err("actual package rejection".into()))
            .unwrap();
        assert_eq!(
            drain(&mut library).err().unwrap(),
            "actual package rejection"
        );
        assert!(!library.busy());
        library.spawn(|| panic!("worker failure check")).unwrap();
        assert_eq!(
            drain(&mut library).err().unwrap(),
            "Library worker panicked"
        );
        assert!(!library.busy());
        assert!(library.result.is_none());
        library
            .spawn(|| {
                Ok(Update::Discovered(Discovery {
                    missing: true,
                    candidates: vec![],
                }))
            })
            .unwrap();
        let Update::Discovered(result) = drain(&mut library).unwrap().unwrap() else {
            panic!("Wrong result")
        };
        assert!(result.missing);
    }

    #[test]
    fn paired_source_import_owns_the_real_worker_and_only_returns_validated_content() {
        let root = std::env::temp_dir().join(format!(
            "cocobeat-source-import-worker-{}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        let inbox = root.join("imports");
        fs::create_dir(&inbox).unwrap();
        fs::write(
            inbox.join("song.ogg"),
            include_bytes!("../../../testdata/synthetic/media-import/stereo-canonical.ogg"),
        )
        .unwrap();
        let authoring = inbox.join("song.authoring.json");
        fs::write(&authoring, r#"{"schema_version":1,"song_id":"worker-import","ruleset_id":"duo-watermark-v1","source_note":"Authored software fixture","anchors":[{"id":5,"frame":1200}],"sections":[]}"#).unwrap();
        let mut library = Library::new(root.clone());
        let discovery = discover_sources(&inbox).unwrap();
        assert_eq!(discovery.candidates.len(), 1);
        let source = discovery.candidates[0].clone();
        let destination = library.new_import_destination().unwrap();
        assert!(!destination.exists());
        library
            .import_authored(source.clone(), destination.clone())
            .unwrap();
        assert!(library.importing());
        assert!(library.load(None).is_err());
        let Update::Loaded(song) = drain(&mut library).unwrap().unwrap() else {
            panic!("Wrong import result")
        };
        assert!(!library.busy());
        assert!(!library.importing());
        assert_eq!(song.path.as_ref(), Some(&destination));
        assert_eq!(song.song_id.as_deref(), Some("worker-import"));
        assert_eq!(song.content.end.frames(), 4800);
        assert_eq!(song.sound.frames.len(), 4800);
        let package = cocobeat_media::validate_package(&destination).unwrap();
        assert_eq!(
            package.analysis.schema_version,
            cocobeat_schema::ANALYSIS_SCHEMA_VERSION
        );
        assert_eq!(package.chart.anchors, song.content.anchors);
        let old: Vec<_> = [
            "song.audio.ogg",
            "analysis.bin",
            "chart.bin",
            "song.package",
        ]
        .map(|name| (name, fs::read(destination.join(name)).unwrap()))
        .into();
        library
            .import_authored(source.clone(), destination.clone())
            .unwrap();
        assert!(drain(&mut library).is_err());
        assert!(
            old.iter()
                .all(|(name, bytes)| fs::read(destination.join(name)).unwrap() == *bytes)
        );
        fs::write(&authoring, b"invalid authoring").unwrap();
        let failed = library.new_import_destination().unwrap();
        library.import_authored(source, failed.clone()).unwrap();
        assert!(drain(&mut library).is_err());
        assert!(!failed.exists());
        assert!(!library.busy());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn bounded_discovery_never_claims_validity_or_follows_directory_links() {
        let root =
            std::env::temp_dir().join(format!("cocobeat-library-check-{}", std::process::id()));
        assert!(!root.exists());
        fs::create_dir(&root).unwrap();
        assert_eq!(
            discover(&root.join("missing")).unwrap(),
            Discovery {
                missing: true,
                candidates: vec![]
            }
        );
        for name in ["z-song", "a-song", "empty", "nested"] {
            fs::create_dir(root.join(name)).unwrap();
        }
        for name in ["z-song", "a-song"] {
            fs::write(
                root.join(name).join("song.package"),
                b"invalid bytes remain discoverable",
            )
            .unwrap();
        }
        fs::create_dir(root.join("nested/hidden-song")).unwrap();
        fs::write(
            root.join("nested/hidden-song/song.package"),
            b"ignored nested",
        )
        .unwrap();
        fs::write(root.join("ordinary-file"), b"ignored").unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("a-song"), root.join("linked-song")).unwrap();
            assert!(discover(&root.join("linked-song")).is_err());
            std::os::unix::fs::symlink(
                root.join("a-song/song.package"),
                root.join("empty/song.package"),
            )
            .unwrap();
        }
        let scan = discover(&root).unwrap();
        assert!(!scan.missing);
        assert_eq!(
            scan.candidates
                .iter()
                .map(|item| item.path.file_name().unwrap().to_str().unwrap())
                .collect::<Vec<_>>(),
            ["a-song", "z-song"]
        );
        assert!(discover(&root.join("ordinary-file")).is_err());
        for index in 0..127 {
            let path = root.join(format!("limit-{index}"));
            fs::create_dir(&path).unwrap();
            fs::write(path.join("song.package"), b"candidate only").unwrap();
        }
        assert!(
            discover(&root)
                .unwrap_err()
                .contains("128 package candidates")
        );
        for index in 0..127 {
            fs::remove_file(root.join(format!("limit-{index}/song.package"))).unwrap();
        }
        for index in 0..512 {
            fs::write(root.join(format!("junk-{index}")), b"").unwrap();
        }
        assert!(
            discover(&root)
                .unwrap_err()
                .contains("512 immediate entries")
        );
        fs::remove_dir_all(&root).unwrap();
    }
}
