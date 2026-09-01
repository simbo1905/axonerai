use env;

fn main() {
    let result = env::set_var("TEST_KEY", "test_value");
    println!("Result: {:?}", result);
}
