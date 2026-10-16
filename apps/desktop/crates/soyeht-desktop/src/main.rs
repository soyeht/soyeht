fn main() {
    // The advice to the person (a missing driver, a missing socket) is text for a terminal, not a
    // log line: this is the one place that prints it.
    soyeht_desktop::run(|text| eprintln!("{text}"));
}
