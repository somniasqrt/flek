//! Terminal input only; normalization and authentication belong to core.

use std::io::{self, BufRead, IsTerminal, Read};

use flek_core::{Error, Result, Secret};
use zeroize::Zeroizing;

pub(super) fn read(json: bool) -> Result<(Secret, Secret)> {
    if io::stdin().is_terminal() {
        let terminal = console::Term::stderr();
        if json || !terminal.is_term() {
            return Err(Error::Usage(
                "pipe two lines via stdin (token, then d cookie); interactive entry requires a terminal on stderr and no --json".into(),
            ));
        }
        let prompt = |label: &str| -> Result<Secret> {
            terminal.write_str(label).map_err(input_error)?;
            terminal
                .read_secure_line()
                .map(Secret::new)
                .map_err(input_error)
        };
        Ok((prompt("Token (hidden): ")?, prompt("d cookie (hidden): ")?))
    } else {
        let mut stdin = io::stdin().lock();
        Ok((read_line(&mut stdin)?, read_line(&mut stdin)?))
    }
}

fn read_line(reader: &mut impl BufRead) -> Result<Secret> {
    // Bound untrusted stdin and wipe the temporary buffer even on EOF/UTF-8 errors.
    const LIMIT: u64 = 64 * 1024;
    let mut line = Zeroizing::new(Vec::new());
    let count = reader
        .take(LIMIT + 1)
        .read_until(b'\n', &mut line)
        .map_err(input_error)?;
    if count == 0 || count as u64 > LIMIT {
        return Err(Error::Usage(
            "expected two bounded lines on stdin: token, then d cookie".into(),
        ));
    }
    let value = std::str::from_utf8(&line)
        .map_err(|_| Error::Usage("manual session input must be UTF-8".into()))?;
    Ok(Secret::new(value.trim_end_matches(['\r', '\n'])))
}

fn input_error(_: io::Error) -> Error {
    Error::Usage("could not read manual session input".into())
}
