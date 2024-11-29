use log::{info, warn, error, LevelFilter};
use simple_logger::SimpleLogger;
use std::io::stdin;

fn fibonacci(n: u64) -> u64 
{
    if n <= 1 
    {
        warn!(fib_n = n; "Sequence index is less than or equal to 1");
        return n;
    }
    let mut a = 0;
    let mut b = 1;
    for _ in 2..=n 
    {
        let temp = a + b;
        a = b;
        b = temp;
    }
    info!(fib_n = n, result = b; "Fibonacci result");
    b
}

fn main()
{
    SimpleLogger::new()
        .with_level(LevelFilter::Info)
        .init()
        .unwrap();

    let mut n_answer = String::new();

    println!("Enter the index of the Fibonacci sequence you want to calculate:");
    match stdin().read_line(&mut n_answer)
    {
        Err(e) =>
        {
            error!("Failed to read input: {}", e);
            return;
        },
        Ok(_) => (),
    }

    let n: u64 = match n_answer.trim().parse::<u64>()
    {
        Err(e) =>
        {
            error!("Invalid number {} [{}], defaulting to 10", n_answer.trim(), e);
            10
        },
        Ok(n) => n,
    };

    info!(fib_n = n; "Welcome to Fib-rs!");

    println!("{}th Fibonacci number is {}", n, fibonacci(n));
}