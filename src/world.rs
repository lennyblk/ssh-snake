//! État partagé : la partie en cours et les comptes.

use std::time::Instant;

use crate::accounts::{Accounts, Stats};
use crate::game::{Game, TOP, clean_name};

pub struct World {
    pub game: Game,
    pub accounts: Accounts,
}

impl World {
    /// Reporte dans les comptes les stats modifiées par la partie.
    pub fn sync(&mut self) {
        if self.game.outbox.is_empty() {
            return;
        }
        for (name, stats) in self.game.outbox.drain(..) {
            self.accounts.update(&name, stats);
        }
        self.game.top = self.accounts.top(TOP);
    }

    pub fn tick(&mut self, now: Instant) {
        self.game.tick(now);
        self.sync();
    }

    /// Entre dans la partie avec le compte de ce pseudo.
    pub fn join(&mut self, id: u64, wanted: &str, now: Instant) -> Result<(), &'static str> {
        self.sync();
        let name = clean_name(wanted);
        if name.is_empty() {
            return Err("choisis un pseudo");
        }
        if self.game.players.iter().any(|p| p.name.eq_ignore_ascii_case(&name)) {
            return Err("ce pseudo est déjà connecté");
        }
        let (name, stats) = match self.accounts.by_name(&name) {
            Some(a) => (a.name.clone(), a.stats.clone()),
            None => (name, Stats::default()),
        };
        self.game.join(id, &name, stats, now);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_name_same_account_across_restart() {
        let t = Instant::now();
        let mut w = World { game: Game::new(), accounts: Accounts::default() };
        w.join(1, "Lenny", t).unwrap();
        assert!(w.join(2, "LENNY", t).is_err());
        assert!(w.join(2, "", t).is_err());
        let stats = Stats { round_wins: 2, ..Default::default() };
        w.game.outbox.push(("Lenny".into(), stats));
        w.tick(t);
        assert_eq!(w.game.top[0].name, "Lenny");
        w.game.leave(1, t);
        w.join(3, "lenny", t).unwrap();
        let p = w.game.player(3).unwrap();
        assert_eq!((p.name.as_str(), p.stats.round_wins), ("Lenny", 2));

        let path = std::env::temp_dir().join(format!("ssh-snake-{}.json", std::process::id()));
        crate::accounts::write_atomic(&path, &w.accounts.to_json()).unwrap();
        assert_eq!(Accounts::load(&path).unwrap().by_name("LENNY").unwrap().stats.round_wins, 2);
        std::fs::remove_file(path).unwrap();
    }
}
