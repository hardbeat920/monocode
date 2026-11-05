// The editor search engine is self-contained apart from these dependencies.
#[path = "../../../../crates/monocode-gpui/src/editor_search.rs"]
mod editor_search;

mod bench;
mod corpus;
mod old;
mod parity;

fn main() {
    if let Err(error) = bench::main() {
        eprintln!("search-bench: {error}");
        std::process::exit(1);
    }
}
