use query::graph::dependency_cycles;
use storage::Database;
fn main() {
    let db = Database::new("/tmp/cycle_test/.codebroker/codebroker.db").unwrap();
    let res = dependency_cycles(&db, 50, None, true).unwrap();
    println!("cycles: {}", res.cycles_returned);
    for c in res.cycles {
        println!("{:?}", c);
    }
}
