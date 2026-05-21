#![forbid(unsafe_code)]

fn main() {
    println!("Hello, world!");
}

#[cfg(test)]
mod tests {
    #[test]
    fn executable_package_metadata_is_declared() {
        assert_eq!(env!("CARGO_PKG_NAME"), "trmnl-whatsapp-list");
    }
}
