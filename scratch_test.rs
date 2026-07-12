use ignore::WalkBuilder;
fn main() {
    for result in WalkBuilder::new("scratch").build() {
        println!("{:?}", result.unwrap().path());
    }
}
