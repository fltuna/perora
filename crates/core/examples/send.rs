//! Signs in if nobody is, then uploads an image file and prints its link. Without the application
//! around it: the address to sign in at is printed, not opened.
//!
//!     cargo run -p perora-core --example send -- image.png
//!     cargo run -p perora-core --example send -- --sign-out
//!
//! The sign-in is kept in the temporary directory, apart from the application's.

use std::io::Write;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use perora_core::account::Account;
use perora_core::config::Config;
use perora_core::platform::token_store;
use perora_core::shots;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let argument = std::env::args()
        .nth(1)
        .ok_or("usage: send <image> | --sign-out")?;
    let config = Config::from_environment();
    println!(
        "server {}, identity provider {}",
        config.server, config.issuer
    );

    let store = token_store(std::env::temp_dir().join("perora-example").join("sign-in"));
    let account = Account::new(config, store);

    if argument == "--sign-out" {
        account.sign_out()?;
        println!("signed out");
        return Ok(());
    }

    if !account.is_signed_in()? {
        let login = account.begin_login()?;
        println!("open: {}", login.authorize_url());
        std::io::stdout().flush()?;
        account.finish_login(login, Duration::from_secs(300), &AtomicBool::new(false))?;
    }

    let session = shots::session(&account)?;
    println!("signed in as {} {:?}", session.name, session.permissions);

    let shot = shots::upload(&account, &std::fs::read(&argument)?)?;
    println!("uploaded: {}", shot.url);
    Ok(())
}
