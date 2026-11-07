//! Tries Perora's updates the way the application does, without installing anything.
//!
//! `cargo run -p perora-core --example release_check -- verify <installer> <signature file> <version>`
//!   checks a signed installer with Perora's key, as an update would before running it: the release
//!   workflow checks what it signed with this, before it publishes it.
//!
//! `cargo run -p perora-core --example release_check -- latest <latest.json> <version>`
//!   reads a latest.json as Perora does, and checks that it is of that version and has an
//!   installer, signed for it, at the address a release of it has: the release workflow checks what
//!   it wrote with this.
//!
//! `cargo run -p perora-core --example release_check -- look <current version> [--fetch]`
//!   asks the releases what is newer than that version and, with `--fetch`, fetches its installer
//!   and checks it (it is not run).

use perora_core::update;

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let result = match arguments.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["verify", installer, signature, version] => verify(installer, signature, version),
        ["latest", latest, version] => latest_of(latest, version),
        ["look", current] => look(current, false),
        ["look", current, "--fetch"] => look(current, true),
        _ => Err("usage: verify <installer> <signature file> <version> | latest <latest.json> <version> | look <current> [--fetch]".into()),
    };
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn verify(installer: &str, signature: &str, version: &str) -> Result<(), String> {
    let bytes = std::fs::read(installer).map_err(|error| format!("{installer}: {error}"))?;
    let signature =
        std::fs::read_to_string(signature).map_err(|error| format!("{signature}: {error}"))?;
    update::verify(&bytes, &signature, update::PUBLIC_KEY, version).map_err(|error| error.to_string())?;
    if !bytes.starts_with(b"MZ") {
        return Err("not an installer".into());
    }
    println!("{installer}: signed by Perora's key, for {version}");
    Ok(())
}

fn latest_of(latest: &str, version: &str) -> Result<(), String> {
    let json = std::fs::read(latest).map_err(|error| format!("{latest}: {error}"))?;
    // (Newer than anything: what every Perora before it reads.)
    let release = update::newer(&json, "0.0.0")
        .map_err(|error| error.to_string())?
        .ok_or("not a release")?;
    if release.version != version.trim_start_matches('v') {
        return Err(format!("{latest} is of {}, not of {version}", release.version));
    }
    let address = format!(
        "https://github.com/fltuna/perora/releases/download/v{0}/Perora_{0}_x64-setup.exe",
        release.version
    );
    if release.url != address {
        return Err(format!("{latest} names {}, not {address}", release.url));
    }
    println!("{latest}: {} at {}", release.version, release.url);
    for language in ["ja", "en"] {
        let changes = release.changes(language).unwrap_or("(none)");
        println!("changes ({language}): {} lines", changes.lines().count());
    }
    Ok(())
}

fn look(current: &str, fetch: bool) -> Result<(), String> {
    let Some(release) = update::newest(current).map_err(|error| error.to_string())? else {
        println!("nothing newer than {current}");
        return Ok(());
    };
    println!("{} at {}", release.version, release.url);
    for language in ["ja", "en"] {
        println!("changes ({language}): {:?}", release.changes(language));
    }
    if fetch {
        let mut last = 0;
        let bytes = update::fetch(&release, |got, of| {
            if got - last >= 1 << 20 || Some(got) == of {
                last = got;
                println!("  {got} of {of:?}");
            }
        })
        .map_err(|error| error.to_string())?;
        println!("fetched {} bytes, signed by Perora's key for {}", bytes.len(), release.version);
    }
    Ok(())
}
