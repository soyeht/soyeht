//! Addresses and names on a network: IPv4, IPv6, hardware addresses, and the names a local
//! network and a tailnet give to a machine.

const IP: &str = "<ip>";
const MAC: &str = "<mac>";
const HOST: &str = "<host>";

/// The endings of a name that a local network or a tailnet gives to a machine.
const HOST_SUFFIXES: [&str; 6] = [
    ".local",
    ".lan",
    ".ts.net",
    ".localdomain",
    ".internal",
    ".home.arpa",
];

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')
}

/// The length of the longest run from `start` made of characters `keep` accepts.
fn run(chars: &[char], start: usize, keep: impl Fn(char) -> bool) -> usize {
    chars[start..].iter().take_while(|c| keep(**c)).count()
}

/// Six pairs of hex digits, separated by colons or by hyphens.
fn mac_len(chars: &[char], start: usize) -> Option<usize> {
    const LENGTH: usize = 17;
    let part = chars.get(start..start + LENGTH)?;
    let separator = part[2];
    let shaped = matches!(separator, ':' | '-')
        && part.iter().enumerate().all(|(index, c)| {
            if index % 3 == 2 {
                *c == separator
            } else {
                c.is_ascii_hexdigit()
            }
        });
    (shaped && !chars.get(start + LENGTH).copied().is_some_and(is_word)).then_some(LENGTH)
}

/// `1.2.3.4`, with a `:port` when one follows.
fn ipv4_len(chars: &[char], start: usize) -> Option<usize> {
    let length = run(chars, start, |c| c.is_ascii_digit() || c == '.');
    let text: String = chars[start..start + length].iter().collect();
    let text = text.trim_end_matches('.');
    let parts: Vec<&str> = text.split('.').collect();
    let shaped = parts.len() == 4
        && parts
            .iter()
            .all(|part| (1..=3).contains(&part.len()) && part.bytes().all(|b| b.is_ascii_digit()));
    let mut end = start + text.len();
    if !shaped || chars.get(end).copied().is_some_and(char::is_alphanumeric) {
        return None;
    }
    if chars.get(end) == Some(&':') {
        end += 1 + run(chars, end + 1, |c| c.is_ascii_digit());
    }
    Some(end - start)
}

/// An IPv6 address: groups of hex digits with a `::`, or five colons or more (a full address has
/// seven), with its `%zone`. A time (`12:34:56`) has neither.
fn ipv6_len(chars: &[char], start: usize) -> Option<usize> {
    let length = run(chars, start, |c| {
        c.is_ascii_hexdigit() || c == ':' || c == '.'
    });
    let text: String = chars[start..start + length].iter().collect();
    let text = text.trim_end_matches('.');
    let colons = text.matches(':').count();
    let groups_fit = text.split(':').all(|group| {
        group.len() <= 4 && group.bytes().all(|b| b.is_ascii_hexdigit())
            // The last group of a mapped address is an IPv4 one.
            || group.contains('.')
    });
    let shaped = text.chars().any(|c| c.is_ascii_hexdigit())
        && (text.contains("::") || colons >= 5)
        && colons >= 2
        && groups_fit;
    let mut end = start + text.chars().count();
    if !shaped || chars.get(end).copied().is_some_and(char::is_alphanumeric) {
        return None;
    }
    if chars.get(end) == Some(&'%') {
        end += 1 + run(chars, end + 1, |c| c.is_ascii_alphanumeric());
    }
    Some(end - start)
}

/// A name that ends like the names of a local network or a tailnet.
fn host_len(chars: &[char], start: usize) -> Option<usize> {
    let length = run(chars, start, is_word);
    let name: String = chars[start..start + length].iter().collect();
    let name = name.trim_end_matches('.').to_ascii_lowercase();
    HOST_SUFFIXES
        .iter()
        .any(|suffix| name.len() > suffix.len() && name.ends_with(suffix))
        .then_some(name.chars().count())
}

pub(super) fn without_addresses(text: &str) -> String {
    super::replace_spans(text, |chars, index| {
        let boundary = index == 0 || !is_word(chars[index - 1]);
        // An address may follow a `_` or `-` (`peer_100.64.12.7`), a name may not.
        let after_joiner =
            index > 0 && !chars[index - 1].is_ascii_alphanumeric() && chars[index - 1] != '.';
        let address = |length: Option<usize>| length.map(|length| (length, IP));
        (boundary || after_joiner)
            .then(|| {
                mac_len(chars, index)
                    .map(|length| (length, MAC))
                    .or_else(|| address(ipv4_len(chars, index)))
                    .or_else(|| address(ipv6_len(chars, index)))
                    .or_else(|| {
                        boundary
                            .then(|| host_len(chars, index))
                            .flatten()
                            .map(|length| (length, HOST))
                    })
            })
            .flatten()
            .map(|(length, label)| (index + length, label))
    })
}
