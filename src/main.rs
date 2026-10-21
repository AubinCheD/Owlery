fn main() {
    let dir = owlery::state::data_dir();
    let loaded = owlery::state::load(&dir);
    println!("Owlery : {} entrée(s) en cache dans {}", loaded.state.inbox.items.len(), dir.display());
}
