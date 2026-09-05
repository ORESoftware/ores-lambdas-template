fn main() -> std::io::Result<()> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let receipt = __CRATE__::adapters::portable::run(stdin.lock(), stdout.lock())?;
    std::process::exit(if receipt.ok { 0 } else { 2 });
}
