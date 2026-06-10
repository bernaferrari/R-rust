use anyhow::Result;
use std::io::{self, Write};

fn main() -> Result<()> {
    println!("rport R interpreter (desktop host)");
    println!("Type R expressions. Enter 'q()' or Ctrl-D to quit.\n");

    let mut session = r_embed::RSession::new().map_err(|e| anyhow::anyhow!(e))?;
    session.enable_host_process_capabilities();

    let mut line_num = 1;
    loop {
        print!("[{line_num}]> ");
        io::stdout().flush()?;

        let mut input = String::new();
        let bytes_read = io::stdin().read_line(&mut input)?;
        if bytes_read == 0 {
            println!();
            break;
        }

        let trimmed = input.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed == "q()" || trimmed == "quit()" {
            break;
        }

        match session.eval(trimmed) {
            Ok(output) => {
                if !output.is_empty() {
                    println!("{output}");
                }
            }
            Err(e) => {
                eprintln!("Error: {e}");
            }
        }

        line_num += 1;
    }

    session.close();
    Ok(())
}
