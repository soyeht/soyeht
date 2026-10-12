//! A size as Finder writes it.

const UNITS: [&str; 6] = ["KB", "MB", "GB", "TB", "PB", "EB"];
const DECIMALS: [u32; 6] = [0, 1, 2, 2, 2, 2];

/// Decimal units, no decimals for KB, one for MB, two above, a trailing zero dropped and a half
/// rounded up: "999 bytes", "12 KB", "1.2 MB", "5.37 GB". The Mac writes the same with its own
/// formatter in the person's locale; the decimal point here is always a dot.
pub fn file_size(bytes: usize) -> String {
    if bytes < 1_000 {
        return format!("{bytes} {}", if bytes == 1 { "byte" } else { "bytes" });
    }
    let bytes = bytes as u128;
    let (mut unit, mut scale) = (0, 1_000u128);
    while bytes / scale >= 1_000 && unit < UNITS.len() - 1 {
        scale *= 1_000;
        unit += 1;
    }
    let mut factor = 10u128.pow(DECIMALS[unit]);
    let mut scaled = (bytes * factor * 2 + scale) / (2 * scale);
    // A value that rounds up to 1000 is one of the next unit.
    if scaled >= 1_000 * factor && unit < UNITS.len() - 1 {
        unit += 1;
        factor = 1;
        scaled = 1;
    }
    let mut text = (scaled / factor).to_string();
    if factor > 1 {
        let digits = DECIMALS[unit] as usize;
        let fraction = format!("{:0digits$}", scaled % factor);
        let fraction = fraction.trim_end_matches('0');
        if !fraction.is_empty() {
            text.push('.');
            text.push_str(fraction);
        }
    }
    format!("{text} {}", UNITS[unit])
}
