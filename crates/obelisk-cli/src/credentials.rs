//! The signed-in Steam account's session, kept in the login Keychain under
//! this program's own service and nowhere else: never in a file, never in an
//! event. Each library keeps its own account.
use crate::{
    events,
    failure::{Failure, KEYCHAIN_UNAVAILABLE, NOT_SIGNED_IN},
    places::IDENTIFIER,
};
use obelisk_bridge::SteamCredentials;
use security_framework::passwords::{
    delete_generic_password, get_generic_password, set_generic_password,
};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// `errSecItemNotFound`: the Keychain holds no such item.
const NOT_FOUND: i32 = -25300;

/// A saved session. Its refresh token signs the account in: deliberately no
/// Debug.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Saved {
    pub account_name: String,
    pub steam_id: u64,
    pub refresh_token: String,
}

impl Saved {
    /// What an event tells of the account: never its token.
    pub fn account(&self) -> events::Account {
        events::Account {
            steam_id: self.steam_id.to_string(),
            account_name: self.account_name.clone(),
        }
    }
}

impl From<SteamCredentials> for Saved {
    fn from(credentials: SteamCredentials) -> Self {
        Self {
            account_name: credentials.account_name,
            steam_id: credentials.steam_id,
            refresh_token: credentials.refresh_token,
        }
    }
}

impl From<Saved> for SteamCredentials {
    fn from(saved: Saved) -> Self {
        Self {
            account_name: saved.account_name,
            steam_id: saved.steam_id,
            refresh_token: saved.refresh_token,
        }
    }
}

/// Where one library's Steam session is kept. Tests replace the Keychain.
pub trait CredentialStore: Send + Sync {
    /// The saved session; `None` when no account is signed in. A failure
    /// tells why the store could not be read.
    fn load(&self) -> Result<Option<Saved>, String>;
    fn save(&self, saved: &Saved) -> Result<(), String>;
    /// Forgets the session. Forgetting none succeeds.
    fn remove(&self) -> Result<(), String>;
}

/// One generic-password item of the login Keychain, holding the session as
/// JSON.
pub struct Keychain {
    service: String,
    account: String,
}

impl Keychain {
    /// The item of `library`'s Steam account: this program's own service,
    /// and the library's path as the account.
    pub fn of(library: &Path) -> Self {
        Self {
            service: format!("{IDENTIFIER}.steam"),
            account: library.to_string_lossy().into_owned(),
        }
    }
}

impl CredentialStore for Keychain {
    fn load(&self) -> Result<Option<Saved>, String> {
        match get_generic_password(&self.service, &self.account) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|_| "the saved Steam session cannot be read".to_owned()),
            Err(error) if error.code() == NOT_FOUND => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    }

    fn save(&self, saved: &Saved) -> Result<(), String> {
        let bytes = serde_json::to_vec(saved).map_err(|error| error.to_string())?;
        set_generic_password(&self.service, &self.account, &bytes)
            .map_err(|error| error.to_string())
    }

    fn remove(&self) -> Result<(), String> {
        match delete_generic_password(&self.service, &self.account) {
            Err(error) if error.code() != NOT_FOUND => Err(error.to_string()),
            _ => Ok(()),
        }
    }
}

/// A Keychain that did not answer, as a command's failure.
pub fn unavailable(reason: String) -> Failure {
    Failure::new(
        KEYCHAIN_UNAVAILABLE,
        format!(
            "The login Keychain did not give or take the Steam session ({}). It is locked, or its item belongs to a build signed otherwise: delete the item of service {IDENTIFIER}.steam, then sign in again.",
            reason.trim_end_matches('.')
        ),
    )
}

/// The account a command needs signed in.
pub fn signed_in(store: &dyn CredentialStore) -> Result<Saved, Failure> {
    store.load().map_err(unavailable)?.ok_or_else(|| {
        Failure::new(
            NOT_SIGNED_IN,
            "No Steam account is signed in: run steam login first.",
        )
    })
}

/// A store in memory, for tests; `Err` once broken.
#[cfg(test)]
#[derive(Default)]
pub struct Memory {
    pub saved: std::sync::Mutex<Option<Saved>>,
    pub broken: std::sync::atomic::AtomicBool,
}

#[cfg(test)]
impl Memory {
    pub fn holding(saved: Saved) -> Self {
        Self {
            saved: std::sync::Mutex::new(Some(saved)),
            broken: Default::default(),
        }
    }

    fn check(&self) -> Result<(), String> {
        match self.broken.load(std::sync::atomic::Ordering::Acquire) {
            true => Err("User interaction is not allowed".into()),
            false => Ok(()),
        }
    }
}

#[cfg(test)]
impl CredentialStore for Memory {
    fn load(&self) -> Result<Option<Saved>, String> {
        self.check()?;
        Ok(self.saved.lock().unwrap().clone())
    }
    fn save(&self, saved: &Saved) -> Result<(), String> {
        self.check()?;
        *self.saved.lock().unwrap() = Some(saved.clone());
        Ok(())
    }
    fn remove(&self) -> Result<(), String> {
        self.check()?;
        *self.saved.lock().unwrap() = None;
        Ok(())
    }
}

#[cfg(test)]
pub fn gordon() -> Saved {
    Saved {
        account_name: "gordon".into(),
        steam_id: 76_561_197_960_265_729,
        refresh_token: "secret-refresh-token".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    #[test]
    fn a_command_that_needs_an_account_gets_the_saved_one_or_fails_as_not_signed_in() {
        let store = Memory::default();
        assert_eq!(signed_in(&store).err().unwrap().code, NOT_SIGNED_IN);
        store.save(&gordon()).unwrap();
        assert!(signed_in(&store).ok().unwrap() == gordon());
        store.remove().unwrap();
        store.remove().unwrap();
        assert_eq!(signed_in(&store).err().unwrap().code, NOT_SIGNED_IN);
    }

    #[test]
    fn a_keychain_that_does_not_answer_is_no_missing_account() {
        let store = Memory::holding(gordon());
        store.broken.store(true, Ordering::Release);
        let failure = signed_in(&store).err().unwrap();
        assert_eq!(failure.code, KEYCHAIN_UNAVAILABLE);
        assert!(!failure.message.contains("secret-refresh-token"));
    }

    #[test]
    fn what_an_event_tells_of_an_account_never_holds_its_token() {
        let told = serde_json::to_string(&gordon().account()).unwrap();
        assert_eq!(
            told,
            r#"{"steam_id":"76561197960265729","account_name":"gordon"}"#
        );
    }

    #[test]
    fn the_session_crosses_to_the_engine_and_back_whole() {
        let credentials = SteamCredentials::from(gordon());
        assert_eq!(credentials.steam_id, 76_561_197_960_265_729);
        assert!(Saved::from(credentials) == gordon());
    }

    /// Writes to the login Keychain, under this program's own service and a
    /// library no command uses: `cargo test -p obelisk-cli -- --ignored`.
    #[test]
    #[ignore = "saves and forgets an item of the login Keychain"]
    fn the_login_keychain_keeps_a_library_s_session_and_forgets_it() {
        let _quiet =
            security_framework::os::macos::keychain::SecKeychain::disable_user_interaction();
        let library = format!("/obelisk-cli-test/{}", std::process::id());
        let keychain = Keychain::of(Path::new(&library));
        keychain.remove().unwrap();
        assert!(keychain.load().unwrap().is_none());
        keychain.save(&gordon()).unwrap();
        assert!(keychain.load().unwrap().unwrap() == gordon());
        let renewed = Saved {
            refresh_token: "another-token".into(),
            ..gordon()
        };
        keychain.save(&renewed).unwrap();
        assert!(keychain.load().unwrap().unwrap() == renewed);
        let other = Keychain::of(Path::new("/obelisk-cli-test/another-library"));
        assert!(other.load().unwrap().is_none(), "each library has its own");
        keychain.remove().unwrap();
        keychain.remove().unwrap();
        assert!(keychain.load().unwrap().is_none());
    }

    #[test]
    fn each_library_keeps_its_account_under_this_program_s_own_service() {
        let keychain = Keychain::of(Path::new("/library"));
        assert_eq!(keychain.service, "cc.orbiters.obelisk-cli.steam");
        assert_eq!(keychain.account, "/library");
        assert!(!keychain.service.contains("zagrios"));
    }
}
