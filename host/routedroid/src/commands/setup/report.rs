//! How `setup` answers: a mistake in how it was run is one line and exit 2;
//! a run that got through says what it changed and what is left to do.

/// A mistake in how setup was run: exit 2, with the message alone.
#[derive(Debug)]
pub struct Usage(pub String);

impl std::fmt::Display for Usage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Usage {}

/// The exit code for `result`, with a usage mistake printed here.
pub fn exit(result: anyhow::Result<i32>) -> anyhow::Result<i32> {
    match result {
        Err(error) => match error.downcast_ref::<Usage>() {
            Some(usage) => {
                eprintln!("error: {usage}");
                Ok(2)
            }
            None => Err(error),
        },
        done => done,
    }
}

pub fn summary(user: &str, done: &[String], left: &[String]) {
    if done.is_empty() {
        println!("\nRoutedroid is already set up for {user}.");
    } else {
        println!("\nRoutedroid is set up for {user}:");
        done.iter().for_each(|line| println!("  - {line}"));
    }
    if left.is_empty() {
        return;
    }
    println!("Next:");
    for (n, line) in left.iter().enumerate() {
        println!("  {}. {line}", n + 1);
    }
}
