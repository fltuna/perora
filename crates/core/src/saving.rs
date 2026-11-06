//! Writing captures to files: one file of each kind the user saves as, all by one name, never over a
//! file that is there (unless the user named it and chose to replace it). From the Tauri version's
//! `preview.rs`, without its window.

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::output::Output;
use crate::platform::Frame;
use crate::settings::Settings;

/// Where "save" writes: the folder the user chose, or one of Perora's own in their pictures. `None`
/// when the system has no pictures folder to offer.
pub fn save_directory(settings: &Settings) -> Option<PathBuf> {
    match &settings.save_directory {
        Some(directory) => Some(directory.clone()),
        None => Some(dirs::picture_dir()?.join("Perora")),
    }
}

/// Not every file was written: which was first, when some are there all the same, the kind that
/// was not, and why.
#[derive(Debug)]
pub struct NotAll {
    pub first: Option<PathBuf>,
    pub kind: &'static str,
    pub error: std::io::Error,
}

/// Writes one file of each kind into the folder, all by one name, and says where the first went.
///
/// Over what is there with `replaces`; what was written before one could not be stays then.
/// Otherwise never over a file: the name is the first that no file of any of the kinds has (a
/// second capture in the same second becomes `… (2)`), and should one not be written, the ones
/// that were are taken away again.
pub fn write_each(
    folder: &Path,
    name: &str,
    files: &[(&'static str, &[u8])],
    replaces: bool,
) -> Result<PathBuf, NotAll> {
    let named = |name: &str| -> Vec<PathBuf> {
        files
            .iter()
            .map(|(kind, _)| folder.join(format!("{name}.{kind}")))
            .collect()
    };
    if replaces {
        let paths = named(name);
        for (at, ((kind, contents), path)) in files.iter().zip(&paths).enumerate() {
            if let Err(error) = std::fs::write(path, contents) {
                let first = (at > 0).then(|| paths[0].clone());
                return Err(NotAll { first, kind, error });
            }
        }
        return Ok(paths.into_iter().next().unwrap_or_default());
    }

    'names: for attempt in 1..=99 {
        let paths = match attempt {
            1 => named(name),
            n => named(&format!("{name} ({n})")),
        };
        if paths.iter().any(|path| path.exists()) {
            continue;
        }
        for (at, ((kind, contents), path)) in files.iter().zip(&paths).enumerate() {
            let created = std::fs::File::create_new(path);
            // (Taken by another meanwhile: that one is not this capture's to take away.)
            let ours = if created.is_ok() { at + 1 } else { at };
            let Err(error) = created.and_then(|mut file| file.write_all(contents)) else {
                continue;
            };
            for written in &paths[..ours] {
                let _ = std::fs::remove_file(written);
            }
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                continue 'names;
            }
            return Err(NotAll {
                first: None,
                kind,
                error,
            });
        }
        return Ok(paths.into_iter().next().unwrap_or_default());
    }
    Err(NotAll {
        first: None,
        kind: files.first().map_or("", |(kind, _)| kind),
        error: std::io::ErrorKind::AlreadyExists.into(),
    })
}

/// Saves a capture as it was taken, as the user has captures saved: every kind of file they save
/// as, all by one name, in the folder for saving (never asking where: the user wants every capture
/// there). `png` is the capture as a PNG already, when there is one. The files written; refused with
/// the reason when one could not be (then none is there).
pub fn save_as_taken(
    settings: &Settings,
    frame: &Frame,
    png: Option<&[u8]>,
) -> Result<Vec<PathBuf>, String> {
    // (As "save" has it, or as the user would rather have these saved.)
    let (kinds, how) = settings.as_taken();
    let directory = save_directory(settings).ok_or("no folder to save to")?;
    // Every kind is made before any is written: one that cannot be made leaves no file behind.
    let files = kinds
        .iter()
        .map(|kind| {
            let how = Output {
                format: *kind,
                ..how
            };
            Ok((
                kind.name(),
                match png {
                    Some(png) if how.keeps(frame.width(), frame.height()) => png.to_vec(),
                    _ => frame.to_file(how)?,
                },
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let files: Vec<_> = files
        .iter()
        .map(|(kind, file)| (*kind, file.as_slice()))
        .collect();
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let name = format!("Perora {}", crate::platform::local_timestamp());
    let first = write_each(&directory, &name, &files, false)
        .map_err(|not_all| not_all.error.to_string())?;
    Ok(files
        .iter()
        .map(|(kind, _)| first.with_extension(kind))
        .collect())
}

/// Takes away files Perora saved, as far as they are still there.
pub fn take_away(files: &[PathBuf]) {
    for file in files {
        if let Err(error) = std::fs::remove_file(file)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!("save: {} was not taken away: {error}", file.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!(
            "perora-saving-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        folder
    }

    #[test]
    fn every_kind_is_written_by_one_name_and_never_over_a_file() {
        let folder = folder("names");
        let files: [(&'static str, &[u8]); 2] = [("png", b"one"), ("jpg", b"two")];
        let first = write_each(&folder, "Perora 2026-11-06 120000", &files, false).unwrap();
        assert_eq!(first, folder.join("Perora 2026-11-06 120000.png"));
        assert_eq!(std::fs::read(folder.join("Perora 2026-11-06 120000.jpg")).unwrap(), b"two");

        // A second in the same second: (2), for both kinds. And when only a file of the other kind
        // has the name, it is not that name either.
        let second = write_each(&folder, "Perora 2026-11-06 120000", &files, false).unwrap();
        assert_eq!(second, folder.join("Perora 2026-11-06 120000 (2).png"));
        std::fs::write(folder.join("Perora 2026-11-06 120000 (3).jpg"), b"other").unwrap();
        let third = write_each(&folder, "Perora 2026-11-06 120000", &files, false).unwrap();
        assert_eq!(third, folder.join("Perora 2026-11-06 120000 (4).png"));
        assert_eq!(
            std::fs::read(folder.join("Perora 2026-11-06 120000 (3).jpg")).unwrap(),
            b"other"
        );
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn replacing_writes_over_what_is_there() {
        let folder = folder("replace");
        std::fs::write(folder.join("shot.png"), b"old").unwrap();
        let files: [(&'static str, &[u8]); 1] = [("png", b"new")];
        write_each(&folder, "shot", &files, true).unwrap();
        assert_eq!(std::fs::read(folder.join("shot.png")).unwrap(), b"new");
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_kind_that_cannot_be_written_takes_the_others_away() {
        let folder = folder("half");
        // A folder where the second file would go: it cannot be created there.
        std::fs::create_dir_all(folder.join("shot.jpg")).unwrap();
        let files: [(&'static str, &[u8]); 2] = [("png", b"one"), ("jpg", b"two")];
        let written = write_each(&folder, "shot", &files, false).unwrap();
        // (The name is taken by the folder: the next one is used, for both.)
        assert_eq!(written, folder.join("shot (2).png"));
        assert!(!folder.join("shot.png").exists());
        let _ = std::fs::remove_dir_all(&folder);
    }
}
