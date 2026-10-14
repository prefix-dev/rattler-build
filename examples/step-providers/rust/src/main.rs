fn main() {
    #[cfg(feature = "enthusiastic")]
    println!("hello from the Rust provider!");
    #[cfg(not(feature = "enthusiastic"))]
    println!("hello from the Rust provider");
}
