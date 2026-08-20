fn main() {
    embuild::espidf::sysenv::output();
    load_secrets();
}

/// Read key=value pairs from the gitignored `.secret` file and expose them to
/// the crate as compile-time environment variables (`env!("SECRET_<KEY>")`).
///
/// Keeps credentials out of source control while still baking them into the
/// firmware image. Missing keys are emitted as empty strings so the crate still
/// compiles; a warning is printed if `.secret` itself is absent.
fn load_secrets() {
    println!("cargo:rerun-if-changed=.secret");

    const KEYS: [&str; 9] = [
        "WIFI_SSID",
        "WIFI_PASSWORD",
        "DB_CLIENT_ID",
        "DB_API_KEY",
        "DB_EVA_NO",
        "GCAL_CLIENT_ID",
        "GCAL_CLIENT_SECRET",
        "GCAL_REFRESH_TOKEN",
        "GCAL_CALENDAR_ID",
    ];

    let content = std::fs::read_to_string(".secret").unwrap_or_else(|_| {
        println!(
            "cargo:warning=.secret not found - copy .secret.example to .secret and fill it in"
        );
        String::new()
    });

    let mut seen = std::collections::HashSet::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim();
            let value = value.trim();
            println!("cargo:rustc-env=SECRET_{key}={value}");
            seen.insert(key.to_string());
        }
    }

    // Guarantee every expected key exists so `env!` always resolves.
    for key in KEYS {
        if !seen.contains(key) {
            println!("cargo:rustc-env=SECRET_{key}=");
        }
    }
}
