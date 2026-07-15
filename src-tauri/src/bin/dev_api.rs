#[tokio::main]
async fn main() {
    if let Err(err) = xiic_book_studio_lib::dev_server::run().await {
        eprintln!("{err}");
        std::process::exit(1);
    }
}
