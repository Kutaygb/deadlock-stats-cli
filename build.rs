fn main() {
    let _ = dotenvy::dotenv();

    let url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:@localhost:5432/deadlock".to_string());
    println!("cargo:rustc-env=DATABASE_URL={}", url);
}

