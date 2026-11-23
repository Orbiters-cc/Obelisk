//! What a release's archive holds, read from the archive itself once its own
//! digest was checked: every member's type, size, mode, link and SHA-256, as
//! bsdtar tells them (an mtree listing of it). A runtime laid from the archive
//! is compared with that, file by file, before it takes the place of another
//! and again in its place.
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs, io,
    os::unix::{ffi::OsStringExt, fs::MetadataExt},
    path::{Path, PathBuf},
    process::Stdio,
};

/// Its owner's bits are what an extraction keeps of a member's mode.
const OWNER: u32 = 0o700;

#[derive(Debug, PartialEq, Eq)]
enum Kind {
    Folder,
    File { size: u64, sha256: String },
    Link(PathBuf),
}

#[derive(Debug, PartialEq, Eq)]
struct Member {
    kind: Kind,
    mode: u32,
}

/// The members of one release's archive, by their path in the release.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Members(BTreeMap<PathBuf, Member>);

impl Members {
    /// The command that tells what `archive` holds.
    pub(super) fn listing(archive: &Path) -> tokio::process::Command {
        let mut read = OsString::from("@");
        read.push(archive);
        let mut command = tokio::process::Command::new("/usr/bin/tar");
        command
            .args(["-cf", "-", "--format=mtree", "--options=!all,type,size,mode,link,sha256"])
            .arg(read)
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        command
    }

    /// What that command told of the archive of the release in `folder`, the
    /// only folder it may hold. None for one that cannot be read to its last
    /// word: what cannot be told is not whole.
    pub(super) fn read(told: &[u8], folder: &str) -> Option<Self> {
        let mut lines = told.strip_suffix(b"\n")?.split(|byte| *byte == b'\n');
        if lines.next()? != b"#mtree" {
            return None;
        }
        let mut members = BTreeMap::new();
        for line in lines {
            let mut words = line.split(|byte| *byte == b' ');
            let path = path(words.next()?, folder)?;
            let (mut kind, mut mode, mut size, mut sha256, mut link) = (None, None, None, None, None);
            for word in words {
                let at = word.iter().position(|byte| *byte == b'=')?;
                let (key, value) = (&word[..at], &word[at + 1..]);
                let text = || std::str::from_utf8(value).ok();
                match key {
                    b"type" => kind = Some(value),
                    b"mode" => mode = Some(u32::from_str_radix(text()?, 8).ok()?),
                    b"size" => size = Some(text()?.parse().ok()?),
                    b"sha256digest" => sha256 = Some(text()?.to_owned()),
                    b"link" => link = Some(PathBuf::from(OsString::from_vec(plain(value)?))),
                    _ => return None,
                }
            }
            let kind = match kind? {
                b"dir" => Kind::Folder,
                b"file" => Kind::File { size: size?, sha256: sha256.filter(|sha| sha.len() == 64)? },
                b"link" => Kind::Link(link?),
                _ => return None,
            };
            members.insert(path, Member { kind, mode: mode? });
        }
        (!members.is_empty()).then_some(Self(members))
    }

    /// The first member that is not in `runtime` as the archive holds it.
    pub(super) fn differing(&self, runtime: &Path) -> Option<&Path> {
        self.0
            .iter()
            .find(|(path, member)| !member.is(&runtime.join(path)).unwrap_or(false))
            .map(|(path, _)| path.as_path())
    }
}

impl Member {
    fn is(&self, path: &Path) -> io::Result<bool> {
        let found = fs::symlink_metadata(path)?;
        Ok(match &self.kind {
            Kind::Link(target) => found.is_symlink() && fs::read_link(path)? == *target,
            Kind::Folder => found.is_dir() && self.allows(&found),
            Kind::File { size, sha256 } => {
                found.is_file() && found.len() == *size && self.allows(&found) && crate::sha(path)? == *sha256
            }
        })
    }

    fn allows(&self, found: &fs::Metadata) -> bool {
        found.mode() & OWNER == self.mode & OWNER
    }
}

/// A member's path in the release's own `folder`.
fn path(told: &[u8], folder: &str) -> Option<PathBuf> {
    let path = PathBuf::from(OsString::from_vec(plain(told)?));
    let path = path.strip_prefix(".").unwrap_or(&path);
    let path = path.strip_prefix(folder).ok()?;
    let inside = path.components().all(|part| matches!(part, std::path::Component::Normal(_)));
    inside.then(|| path.to_owned())
}

/// A name as the listing writes it: what is not plain in octal.
fn plain(told: &[u8]) -> Option<Vec<u8>> {
    let mut name = Vec::new();
    let mut rest = told;
    while let Some((&byte, after)) = rest.split_first() {
        rest = after;
        if byte != b'\\' {
            name.push(byte);
            continue;
        }
        let digits = after.get(..3).filter(|digits| digits.iter().all(|digit| matches!(digit, b'0'..=b'7')))?;
        name.push(u8::try_from(digits.iter().fold(0_u32, |value, digit| value * 8 + u32::from(digit - b'0'))).ok()?);
        rest = &after[3..];
    }
    Some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(told: &str) -> Option<Members> {
        Members::read(told.as_bytes(), "voile-v1.0.0-2")
    }

    /// What bsdtar tells of a member, whatever its characters.
    #[test]
    fn a_member_is_read_as_the_listing_writes_it() {
        let sha = "a".repeat(64);
        let told = format!(
            "#mtree\n./voile-v1.0.0-2 mode=755 type=dir\n./voile-v1.0.0-2/e\\314\\201\\040\\346\\227\\245/a\\134b mode=644 type=file size=3 sha256digest={sha}\n./voile-v1.0.0-2/l mode=755 type=link link=../a\\040b\n"
        );
        let members = read(&told).unwrap().0;
        assert_eq!(members[Path::new("")], Member { kind: Kind::Folder, mode: 0o755 });
        let file = &members[Path::new("e\u{301} 日/a\\b")];
        assert_eq!((&file.kind, file.mode), (&Kind::File { size: 3, sha256: sha }, 0o644));
        assert_eq!(members[Path::new("l")].kind, Kind::Link("../a b".into()));
    }

    #[test]
    fn what_cannot_be_read_is_no_list_of_members() {
        let sha = "a".repeat(64);
        for told in [
            String::new(),
            "#mtree\n".to_owned(),
            "#mtree\n./voile-v1.0.0-2 mode=755 type=dir".to_owned(),
            "./voile-v1.0.0-2 mode=755 type=dir\n".to_owned(),
            "#mtree\n./elsewhere/a mode=755 type=dir\n".to_owned(),
            "#mtree\n./voile-v1.0.0-2/../a mode=755 type=dir\n".to_owned(),
            "#mtree\n./voile-v1.0.0-2/a mode=755 type=fifo\n".to_owned(),
            "#mtree\n./voile-v1.0.0-2/a type=dir\n".to_owned(),
            "#mtree\n./voile-v1.0.0-2/a mode=755 type=link\n".to_owned(),
            // a hard link to another member tells no digest
            "#mtree\n./voile-v1.0.0-2/a mode=755 type=file size=3\n".to_owned(),
            format!("#mtree\n./voile-v1.0.0-2/a mode=755 type=file sha256digest={sha}\n"),
            "#mtree\n./voile-v1.0.0-2/a mode=755 type=dir what\n".to_owned(),
            "#mtree\n./voile-v1.0.0-2/a\\q mode=755 type=dir\n".to_owned(),
        ] {
            assert_eq!(read(&told), None, "{told}");
        }
    }
}
