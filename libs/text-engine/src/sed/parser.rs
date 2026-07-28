use alloc::string::String;

use super::pattern::SedPattern;
use super::replacement::Replacement;
use super::SedError;

pub struct SedScript {
    pub command: Command,
}

pub enum Command {
    Substitute {
        pattern: SedPattern,
        replacement: Replacement,
        global: bool,
        print: bool,
    },
    Delete(Address),
    Print(Address),
}

pub enum Address {
    Pattern(SedPattern),
    Line(usize),
}

impl Address {
    pub fn matches(&self, line: &str, line_number: usize) -> bool {
        match self {
            Self::Pattern(pattern) => pattern.is_match(line),
            Self::Line(target) => *target == line_number,
        }
    }
}

pub fn parse(script: &str) -> Result<SedScript, SedError> {
    if script.is_empty() {
        return Err(SedError::Parse("missing script"));
    }
    if script.contains(';') || script.contains('\n') {
        return Err(SedError::Parse("multi-command scripts are not supported"));
    }
    if matches!(
        script.as_bytes()[0],
        b':' | b'b' | b't' | b'h' | b'H' | b'g' | b'G' | b'x' | b'y' | b'r' | b'w'
    ) {
        return Err(SedError::Parse("unsupported sed command"));
    }
    if script.starts_with('s') {
        return parse_substitute(script);
    }
    if let Some(line) = script.strip_suffix('p') {
        if !line.is_empty() && line.bytes().all(|byte| byte.is_ascii_digit()) {
            let line = line
                .parse::<usize>()
                .map_err(|_| SedError::Parse("invalid line address"))?;
            if line == 0 {
                return Err(SedError::Parse("invalid line address"));
            }
            return Ok(SedScript {
                command: Command::Print(Address::Line(line)),
            });
        }
    }
    parse_address(script)
}

fn parse_substitute(script: &str) -> Result<SedScript, SedError> {
    let delim = script
        .chars()
        .nth(1)
        .ok_or(SedError::Parse("missing substitute delimiter"))?;
    let (pattern, next) = scan_segment(script, 2, delim)?;
    let (replacement, next) = scan_segment(script, next, delim)?;
    let flags = &script[next..];
    let mut global = false;
    let mut print = false;
    for flag in flags.bytes() {
        match flag {
            b'g' if !global => global = true,
            b'p' if !print => print = true,
            _ => return Err(SedError::Parse("invalid substitute flag")),
        }
    }
    Ok(SedScript {
        command: Command::Substitute {
            pattern: SedPattern::compile(&pattern, delim)?,
            replacement: Replacement::compile(&replacement, delim)?,
            global,
            print,
        },
    })
}

fn parse_address(script: &str) -> Result<SedScript, SedError> {
    if !script.starts_with('/') {
        return Err(SedError::Parse("unsupported sed command"));
    }
    let (pattern, next) = scan_segment(script, 1, '/')?;
    match script.as_bytes()[next..] {
        [b'p'] => Ok(SedScript {
            command: Command::Print(Address::Pattern(SedPattern::compile(&pattern, '/')?)),
        }),
        [b'd'] => Ok(SedScript {
            command: Command::Delete(Address::Pattern(SedPattern::compile(&pattern, '/')?)),
        }),
        _ => Err(SedError::Parse("invalid address command")),
    }
}

fn scan_segment(script: &str, start: usize, delim: char) -> Result<(String, usize), SedError> {
    let bytes = script.as_bytes();
    let delim = delim as u8;
    let mut idx = start;
    let mut out = String::new();
    while let Some(&byte) = bytes.get(idx) {
        if byte == delim {
            return Ok((out, idx + 1));
        }
        if byte == b'\\' {
            let next = *bytes
                .get(idx + 1)
                .ok_or(SedError::Parse("unterminated escape"))?;
            out.push('\\');
            out.push(next as char);
            idx += 2;
            continue;
        }
        out.push(byte as char);
        idx += 1;
    }
    Err(SedError::Parse("unterminated sed segment"))
}

#[cfg(test)]
mod tests {
    use super::{parse, Address, Command};
    use core::matches;

    #[test]
    fn parses_substitute_flags_and_alt_delimiter() {
        let script = parse(r"s|a\|b|x\\y|pg").expect("script parses");
        match script.command {
            Command::Substitute { global, print, .. } => {
                assert!(global);
                assert!(print);
            }
            _ => panic!("expected substitute"),
        }
    }

    #[test]
    fn parses_numeric_and_regex_addresses() {
        let numeric = parse("7p").expect("numeric address parses");
        assert!(matches!(numeric.command, Command::Print(Address::Line(7))));
        let regex = parse(r"/a\/b/d").expect("regex address parses");
        assert!(matches!(
            regex.command,
            Command::Delete(Address::Pattern(_))
        ));
    }

    #[test]
    fn rejects_unsupported_and_multicommand_scripts() {
        assert!(parse("w file").is_err());
        assert!(parse("s/a/b/;p").is_err());
    }
}
