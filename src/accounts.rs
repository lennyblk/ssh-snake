//! Comptes : un pseudo = un compte, créé à la première manche jouée.
//! Aucune preuve d'identité : c'est un jeu entre amis.

use std::io::Write;
use std::path::Path;
use std::{fs, io};

use serde::{Deserialize, Serialize};

const MAX_ACCOUNTS: usize = 5000;

#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Stats {
    pub rounds: u64,
    pub round_wins: u64,
    pub match_wins: u64,
    pub kills: u64,
    /// Plus grande longueur atteinte.
    pub longest: u32,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Account {
    pub name: String,
    #[serde(default)]
    pub stats: Stats,
}

#[derive(Default)]
pub struct Accounts {
    list: Vec<Account>,
    pub dirty: bool,
}

impl Accounts {
    pub fn load(path: &Path) -> io::Result<Self> {
        let list = match fs::read(path) {
            Ok(data) => serde_json::from_slice(&data)?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e),
        };
        Ok(Accounts { list, dirty: false })
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(&self.list).expect("comptes sérialisables")
    }

    pub fn by_name(&self, name: &str) -> Option<&Account> {
        self.list.iter().find(|a| a.name.eq_ignore_ascii_case(name))
    }

    /// Classement par matchs gagnés, puis manches gagnées.
    pub fn top(&self, n: usize) -> Vec<Account> {
        let mut v: Vec<&Account> = self.list.iter().filter(|a| a.stats.round_wins > 0).collect();
        v.sort_by_key(|a| std::cmp::Reverse((a.stats.match_wins, a.stats.round_wins)));
        v.into_iter().take(n).cloned().collect()
    }

    pub fn update(&mut self, name: &str, stats: Stats) {
        let full = self.list.len() >= MAX_ACCOUNTS;
        match self.list.iter_mut().find(|a| a.name.eq_ignore_ascii_case(name)) {
            Some(a) => a.stats = stats,
            None if !full => self.list.push(Account { name: name.into(), stats }),
            None => return,
        }
        self.dirty = true;
    }
}

/// Écrit à côté puis renomme : jamais de fichier à moitié écrit.
pub fn write_atomic(path: &Path, text: &str) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    let mut f = fs::File::create(&tmp)?;
    f.write_all(text.as_bytes())?;
    f.sync_all()?;
    fs::rename(&tmp, path)
}
