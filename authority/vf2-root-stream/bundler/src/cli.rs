use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;

pub type CliResult<T> = Result<T, String>;

pub struct Flags {
    values: BTreeMap<String, String>,
}

impl Flags {
    pub fn parse<I>(args: I) -> CliResult<Self>
    where
        I: IntoIterator<Item = OsString>,
    {
        let mut iter = args.into_iter();
        let _program = iter.next();
        let mut values = BTreeMap::new();
        while let Some(raw_name) = iter.next() {
            let name = raw_name
                .into_string()
                .map_err(|_| "argument names must be UTF-8".to_owned())?;
            if !name.starts_with("--") || name.len() == 2 {
                return Err(format!("expected --name, found {name:?}"));
            }
            let raw_value = iter
                .next()
                .ok_or_else(|| format!("missing value for {name}"))?;
            let value = raw_value
                .into_string()
                .map_err(|_| format!("value for {name} must be UTF-8"))?;
            if value.starts_with("--") {
                return Err(format!("missing value for {name}"));
            }
            if values.insert(name.clone(), value).is_some() {
                return Err(format!("duplicate argument {name}"));
            }
        }
        Ok(Self { values })
    }

    pub fn take(&mut self, name: &str) -> CliResult<String> {
        self.values
            .remove(name)
            .ok_or_else(|| format!("missing required argument {name}"))
    }

    pub fn path(&mut self, name: &str) -> CliResult<PathBuf> {
        let value = self.take(name)?;
        if value.is_empty() {
            return Err(format!("{name} must not be empty"));
        }
        Ok(PathBuf::from(value))
    }

    pub fn u64(&mut self, name: &str) -> CliResult<u64> {
        let value = self.take(name)?;
        parse_u64(name, &value)
    }

    pub fn nonzero(&mut self, name: &str) -> CliResult<u64> {
        let value = self.u64(name)?;
        if value == 0 {
            return Err(format!("{name} must be nonzero"));
        }
        Ok(value)
    }

    pub fn hex32(&mut self, name: &str) -> CliResult<[u8; 32]> {
        let value = self.take(name)?;
        if value.len() != 64 {
            return Err(format!("{name} must be exactly 64 hexadecimal characters"));
        }
        let mut out = [0; 32];
        for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
            out[index] = (nibble(name, pair[0])? << 4) | nibble(name, pair[1])?;
        }
        Ok(out)
    }

    pub fn finish(self) -> CliResult<()> {
        match self.values.keys().next() {
            Some(name) => Err(format!("unknown argument {name}")),
            None => Ok(()),
        }
    }
}

fn parse_u64(name: &str, value: &str) -> CliResult<u64> {
    let parsed = match value.strip_prefix("0x") {
        Some(hex) if !hex.is_empty() => u64::from_str_radix(hex, 16),
        Some(_) => return Err(format!("{name} has an empty hexadecimal value")),
        None => value.parse(),
    };
    parsed
        .map_err(|_| format!("{name} must be a decimal integer or 0x-prefixed hexadecimal integer"))
}

fn nibble(name: &str, value: u8) -> CliResult<u8> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        b'A'..=b'F' => Ok(value - b'A' + 10),
        _ => Err(format!("{name} must be exactly 64 hexadecimal characters")),
    }
}
