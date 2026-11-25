//! Salon, match en plusieurs manches et chat. Le temps est toujours passé en paramètre.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::accounts::{Account, Stats};
use crate::arena::{Arena, Dir, STEPS_PER_SEC};

pub const COUNTDOWN: Duration = Duration::from_secs(10);
pub const SOLO_COUNTDOWN: Duration = Duration::from_secs(2);
pub const JOIN_GRACE: Duration = Duration::from_secs(5);
pub const MAX_COUNTDOWN: Duration = Duration::from_secs(30);
/// Arène affichée, serpents immobiles : le temps de se repérer.
pub const READY: Duration = Duration::from_secs(4);
pub const ROUND_END: Duration = Duration::from_secs(4);
pub const RESULTS: Duration = Duration::from_secs(8);
/// Manches pour gagner le match, et garde-fou contre les nuls à répétition.
pub const ROUNDS_TO_WIN: u32 = 3;
const MAX_ROUNDS: u32 = 9;
/// Serpents par manche ; au-delà, spectateurs, prioritaires au match suivant.
pub const MAX_SNAKES: usize = 24;

pub const MAX_NAME: usize = 16;
pub const MAX_MESSAGE: usize = 120;
pub const COLORS: usize = 16;
pub const TOP: usize = 8;
const CHAT_HISTORY: usize = 50;
/// Chat fermé avant le départ et juste après la fin : les commandes du jeu
/// ne doivent pas tomber dans le chat, ni l'inverse.
const CHAT_LOCK: Duration = Duration::from_secs(3);
const CHAT_REOPEN: Duration = Duration::from_millis(1500);
const CHAT_RATE: f64 = 0.5;
const CHAT_BURST: f64 = 4.0;

const STEP: Duration = Duration::from_millis(1000 / STEPS_PER_SEC);

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Phase {
    Lobby { start_at: Option<Instant> },
    Ready { until: Instant },
    Round { next_step: Instant },
    RoundEnd { until: Instant },
    Results { until: Instant },
}

#[derive(Clone)]
pub struct Player {
    pub id: u64,
    pub name: String,
    /// Indice de couleur (serpent et chat), libre parmi les connectés.
    pub color: usize,
    pub ready: bool,
    /// Manches gagnées dans le match en cours.
    pub points: u32,
    pub stats: Arc<Stats>,
    /// A joué, et a touché une commande, pendant le match en cours.
    in_match: bool,
    active: bool,
    chat_tokens: f64,
    last_chat: Instant,
}

#[derive(Clone)]
pub struct Message {
    pub color: usize,
    pub name: String,
    pub text: String,
}

#[derive(Clone)]
pub struct Game {
    pub phase: Phase,
    pub players: Vec<Player>,
    /// Arène en cours, ou figée pendant les écrans de fin.
    pub arena: Option<Arena>,
    pub round: u32,
    /// Vainqueur de la manche, puis du match sur l'écran de résultats.
    pub round_winner: Option<String>,
    pub champion: Option<String>,
    pub chat: VecDeque<Message>,
    pub top: Vec<Account>,
    /// Stats à reporter dans les comptes (voir `World::sync`).
    pub outbox: Vec<(String, Stats)>,
    solo: bool,
    latest_start: Instant,
}

pub fn clean_name(raw: &str) -> String {
    raw.chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        .take(MAX_NAME)
        .collect()
}

/// Latin d'une colonne de large : ni contrôle, ni bidi, ni caractère combinant.
pub fn chat_char(c: char) -> bool {
    matches!(c, ' '..='~' | '\u{a1}'..='\u{ac}' | '\u{ae}'..='\u{24f}' | '€' | '’')
}

impl Game {
    pub fn new() -> Self {
        Game {
            phase: Phase::Lobby { start_at: None },
            players: Vec::new(),
            arena: None,
            round: 0,
            round_winner: None,
            champion: None,
            chat: VecDeque::new(),
            top: Vec::new(),
            outbox: Vec::new(),
            solo: false,
            latest_start: Instant::now(),
        }
    }

    pub fn player(&self, id: u64) -> Option<&Player> {
        self.players.iter().find(|p| p.id == id)
    }

    /// Le pseudo doit être propre et libre (voir `World::join`).
    pub fn join(&mut self, id: u64, name: &str, stats: Stats, now: Instant) {
        // Couleur la moins récemment vue dans le chat parmi celles qui sont libres.
        let color = (0..)
            .filter(|c| self.players.iter().all(|p| p.color != *c))
            .take(COLORS.saturating_sub(self.players.len()).max(1))
            .min_by_key(|c| self.chat.iter().rposition(|m| m.color == *c).map_or(0, |i| i + 1))
            .unwrap();
        self.players.push(Player {
            id,
            name: name.to_string(),
            color,
            ready: true,
            points: 0,
            stats: Arc::new(stats),
            in_match: false,
            active: false,
            chat_tokens: CHAT_BURST,
            last_chat: now,
        });
        self.tick(now);
        if let Phase::Lobby { start_at: Some(t) } = self.phase {
            let t = t.max((now + JOIN_GRACE).min(self.latest_start));
            self.phase = Phase::Lobby { start_at: Some(t) };
        }
    }

    pub fn leave(&mut self, id: u64, now: Instant) {
        self.players.retain(|p| p.id != id);
        if matches!(self.phase, Phase::Ready { .. } | Phase::Round { .. })
            && let Some(a) = &mut self.arena
        {
            a.leave(id);
        }
        self.tick(now);
    }

    pub fn ready_count(&self) -> usize {
        self.players.iter().filter(|p| p.ready).count()
    }

    /// Entrée dans le salon : départ solo si seul prêt, sinon bascule prêt / pas prêt.
    pub fn press_enter(&mut self, id: u64, now: Instant) {
        if !matches!(self.phase, Phase::Lobby { .. }) {
            return;
        }
        let ready = self.ready_count();
        let Some(p) = self.players.iter_mut().find(|p| p.id == id) else { return };
        if p.ready && ready == 1 && !self.solo {
            self.solo = true;
        } else {
            p.ready = !p.ready;
        }
        self.tick(now);
    }

    /// Seul connecté : peut relancer à tout moment.
    pub fn alone(&self) -> bool {
        self.players.len() == 1
    }

    pub fn restart(&mut self, id: u64, now: Instant) {
        if self.alone() && self.players[0].id == id {
            self.players[0].ready = true;
            self.start_match(now);
        }
    }

    pub fn chat_open(&self, now: Instant) -> bool {
        match self.phase {
            Phase::Lobby { start_at: None } => true,
            Phase::Lobby { start_at: Some(t) } => t.saturating_duration_since(now) > CHAT_LOCK,
            Phase::Results { until } => until.saturating_duration_since(now) <= RESULTS - CHAT_REOPEN,
            _ => false,
        }
    }

    /// Publie un message. Faux si le chat est fermé ou le joueur trop bavard.
    pub fn say(&mut self, id: u64, text: &str, now: Instant) -> bool {
        if !self.chat_open(now) {
            return false;
        }
        let Some(p) = self.players.iter_mut().find(|p| p.id == id) else { return false };
        let dt = now.saturating_duration_since(p.last_chat).as_secs_f64();
        p.last_chat = now;
        p.chat_tokens = (p.chat_tokens + dt * CHAT_RATE).min(CHAT_BURST);
        if p.chat_tokens < 1.0 {
            return false;
        }
        p.chat_tokens -= 1.0;
        let text = text.trim().chars().filter(|&c| chat_char(c)).take(MAX_MESSAGE).collect();
        self.chat.push_back(Message { color: p.color, name: p.name.clone(), text });
        if self.chat.len() > CHAT_HISTORY {
            self.chat.pop_front();
        }
        true
    }

    /// Accepté aussi pendant le décompte, pour partir dans la bonne direction.
    pub fn steer(&mut self, id: u64, dir: Dir) {
        if matches!(self.phase, Phase::Ready { .. } | Phase::Round { .. })
            && let Some(a) = &mut self.arena
        {
            a.steer(id, dir);
        }
    }

    pub fn boost(&mut self, id: u64) {
        if matches!(self.phase, Phase::Round { .. })
            && let Some(a) = &mut self.arena
        {
            a.boost(id);
        }
    }

    fn update_stats(&mut self, id: u64, f: impl FnOnce(&mut Stats)) {
        if let Some(p) = self.players.iter_mut().find(|p| p.id == id) {
            f(Arc::make_mut(&mut p.stats));
            self.outbox.push((p.name.clone(), Stats::clone(&p.stats)));
        }
    }

    /// Fait avancer la machine à états. Idempotent.
    pub fn tick(&mut self, now: Instant) {
        match self.phase {
            Phase::Lobby { start_at } => {
                let ready = self.ready_count();
                if ready != 1 {
                    self.solo = false;
                }
                match start_at {
                    _ if ready < 2 && !self.solo => self.phase = Phase::Lobby { start_at: None },
                    None => {
                        let wait = if self.solo { SOLO_COUNTDOWN } else { COUNTDOWN };
                        self.latest_start = now + MAX_COUNTDOWN;
                        self.phase = Phase::Lobby { start_at: Some(now + wait) };
                    }
                    Some(t) if now >= t => self.start_match(now),
                    Some(_) => {}
                }
            }
            Phase::Ready { until } => {
                if self.arena.as_ref().is_none_or(|a| a.snakes.is_empty()) {
                    self.to_lobby();
                } else if now >= until {
                    self.phase = Phase::Round { next_step: now + STEP };
                }
            }
            Phase::Round { mut next_step } => {
                let Some(a) = &mut self.arena else { return self.to_lobby() };
                // Après un blocage du serveur, on ne rattrape pas plus de 4 pas.
                next_step = next_step.max(now.checked_sub(STEP * 4).unwrap_or(now));
                while now >= next_step && !a.over() {
                    a.advance();
                    next_step += STEP;
                }
                let over = a.over();
                self.phase = Phase::Round { next_step };
                if over {
                    self.end_round(now);
                }
            }
            Phase::RoundEnd { until } => {
                if now >= until {
                    self.start_round(now);
                }
            }
            Phase::Results { until } => {
                if now >= until {
                    self.to_lobby();
                    self.tick(now);
                }
            }
        }
    }

    fn to_lobby(&mut self) {
        for p in self.players.iter_mut().filter(|p| p.in_match) {
            p.ready = p.active;
        }
        // Les spectateurs passent devant pour le prochain match ; tri stable.
        self.players.sort_by_key(|p| p.in_match);
        self.reset_match();
        self.solo = false;
        self.arena = None;
        self.phase = Phase::Lobby { start_at: None };
    }

    fn reset_match(&mut self) {
        for p in &mut self.players {
            p.points = 0;
            p.in_match = false;
            p.active = false;
        }
        self.round = 0;
        self.round_winner = None;
        self.champion = None;
    }

    fn start_match(&mut self, now: Instant) {
        self.reset_match();
        self.solo = false;
        self.start_round(now);
    }

    fn start_round(&mut self, now: Instant) {
        let first = self.round == 0;
        self.round += 1;
        self.round_winner = None;
        // Participants du match d'abord, puis les nouveaux venus tant qu'il reste de la place.
        let mut ids: Vec<u64> = self.players.iter().filter(|p| p.in_match).map(|p| p.id).collect();
        let new = self.players.iter().filter(|p| p.ready && !p.in_match).map(|p| p.id);
        ids.extend(new);
        ids.truncate(MAX_SNAKES);
        // Seul restant d'un match à plusieurs : plus d'adversaire.
        if ids.is_empty() || (!first && ids.len() < 2) {
            self.to_lobby();
            return;
        }
        let mut snakes = Vec::new();
        for p in self.players.iter_mut().filter(|p| ids.contains(&p.id)) {
            p.in_match = true;
            snakes.push((p.id, p.name.clone(), p.color));
        }
        self.arena = Some(Arena::new(&snakes));
        self.phase = Phase::Ready { until: now + READY };
    }

    fn end_round(&mut self, now: Instant) {
        let Some(a) = self.arena.clone() else { return self.to_lobby() };
        let multi = a.snakes.len() > 1 || self.round > 1 || a.winner().is_some();
        let winner = a.winner().map(|s| s.id);
        self.round_winner = a.winner().map(|s| s.name.clone());
        for s in &a.snakes {
            if let Some(p) = self.players.iter_mut().find(|p| p.id == s.id) {
                p.active |= s.active;
                p.points += (winner == Some(s.id)) as u32;
            }
            // Un absent n'a pas de stats, mais un vainqueur resté immobile garde son point.
            if s.active {
                let won = multi && winner == Some(s.id);
                self.update_stats(s.id, |st| {
                    st.rounds += 1;
                    st.round_wins += won as u64;
                    st.kills += s.kills as u64;
                    st.longest = st.longest.max(s.longest as u32);
                });
            }
        }
        let best = self.players.iter().map(|p| p.points).max().unwrap_or(0);
        let anyone = a.snakes.iter().any(|s| s.active);
        let left = self.players.iter().filter(|p| p.in_match).count();
        let done = !multi || best >= ROUNDS_TO_WIN || self.round >= MAX_ROUNDS || !anyone || left < 2;
        if !done {
            self.phase = Phase::RoundEnd { until: now + ROUND_END };
            return;
        }
        let leaders: Vec<(u64, String, bool)> = self
            .players
            .iter()
            .filter(|p| p.points == best && best > 0)
            .map(|p| (p.id, p.name.clone(), p.active))
            .collect();
        // Égalité en tête : pas de champion. Match abandonné : pas de victoire comptée.
        if let [(id, name, active)] = leaders.as_slice() {
            self.champion = Some(name.clone());
            if *active && best >= ROUNDS_TO_WIN {
                self.update_stats(*id, |s| s.match_wins += 1);
            }
        }
        self.phase = Phase::Results { until: now + RESULTS };
    }

    /// Joueurs du match, du meilleur au moins bon.
    pub fn scoreboard(&self) -> Vec<&Player> {
        let mut v: Vec<&Player> = self.players.iter().filter(|p| p.in_match).collect();
        v.sort_by_key(|p| std::cmp::Reverse(p.points));
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(s: u64) -> Duration {
        Duration::from_secs(s)
    }

    fn game(n: u64) -> (Game, Instant) {
        let t = Instant::now();
        let mut g = Game::new();
        for id in 1..=n {
            g.join(id, &format!("p{id}"), Stats::default(), t);
        }
        (g, t)
    }

    fn in_round(n: u64) -> (Game, Instant) {
        let (mut g, t) = game(n);
        let t = t + MAX_COUNTDOWN;
        g.tick(t);
        let t = t + READY;
        g.tick(t);
        assert!(matches!(g.phase, Phase::Round { .. }));
        (g, t)
    }

    /// Tue tous les serpents sauf celui de `winner`.
    fn win(g: &mut Game, winner: u64, t: Instant) {
        let a = g.arena.as_mut().unwrap();
        for s in &mut a.snakes {
            s.active = true;
            if s.id != winner {
                s.alive = false;
                s.body.clear();
            }
        }
        g.tick(t);
    }

    #[test]
    fn first_to_three_wins_the_match() {
        let (mut g, mut t) = in_round(3);
        for (n, w) in [1, 2, 1, 1].into_iter().enumerate() {
            win(&mut g, w, t);
            if n < 3 {
                assert!(matches!(g.phase, Phase::RoundEnd { .. }));
                t += ROUND_END;
                g.tick(t);
                t += READY;
                g.tick(t);
            }
        }
        assert!(matches!(g.phase, Phase::Results { .. }));
        assert_eq!(g.champion.as_deref(), Some("p1"));
        let s = &g.player(1).unwrap().stats;
        assert_eq!((s.rounds, s.round_wins, s.match_wins), (4, 3, 1));
        g.tick(t + RESULTS);
        assert!(matches!(g.phase, Phase::Lobby { start_at: Some(_) }));
        assert!(g.players.iter().all(|p| p.points == 0));
    }

    #[test]
    fn newcomer_enters_next_round() {
        let (mut g, t) = in_round(2);
        g.join(9, "eve", Stats::default(), t);
        assert!(g.arena.as_ref().unwrap().snake(9).is_none());
        win(&mut g, 1, t);
        g.tick(t + ROUND_END);
        assert!(g.arena.as_ref().unwrap().snake(9).is_some());
    }

    #[test]
    fn solo_run_restart_and_no_win_farming() {
        let (mut g, t) = game(1);
        assert_eq!(g.phase, Phase::Lobby { start_at: None });
        g.press_enter(1, t);
        g.tick(t + SOLO_COUNTDOWN);
        let t = t + SOLO_COUNTDOWN + READY;
        g.tick(t);
        g.steer(1, Dir::Up);
        g.steer(1, Dir::Left);
        let mut end = t;
        while matches!(g.phase, Phase::Round { .. }) {
            end += STEP;
            g.tick(end);
        }
        assert!(matches!(g.phase, Phase::Results { .. }));
        let s = &g.player(1).unwrap().stats;
        assert_eq!((s.round_wins, s.match_wins), (0, 0));
        g.restart(1, end);
        assert!(matches!(g.phase, Phase::Ready { .. }));
        let (mut g, t) = in_round(2);
        g.restart(1, t);
        assert!(matches!(g.phase, Phase::Round { .. }));
    }

    #[test]
    fn leaving_everyone_gives_point_but_no_match() {
        let (mut g, t) = in_round(2);
        g.steer(1, Dir::Up);
        g.leave(2, t);
        assert!(matches!(g.phase, Phase::Results { .. }));
        assert_eq!(g.player(1).unwrap().stats.match_wins, 0);
        g.leave(1, t);
        g.tick(t + RESULTS);
        assert_eq!(g.phase, Phase::Lobby { start_at: None });
    }

    #[test]
    fn idle_match_stops_and_absents_are_not_ready() {
        let (mut g, mut t) = in_round(2);
        while matches!(g.phase, Phase::Round { .. }) {
            t += STEP;
            g.tick(t);
        }
        assert!(matches!(g.phase, Phase::Results { .. }));
        g.tick(t + RESULTS);
        assert_eq!(g.phase, Phase::Lobby { start_at: None });
    }

    #[test]
    fn extra_players_spectate_then_go_first() {
        let (mut g, t) = in_round(30);
        assert_eq!(g.arena.as_ref().unwrap().snakes.len(), MAX_SNAKES);
        let mut t = t;
        for _ in 0..ROUNDS_TO_WIN {
            win(&mut g, 1, t);
            t += RESULTS;
            g.tick(t);
            t += READY;
            g.tick(t);
        }
        t += MAX_COUNTDOWN;
        g.tick(t);
        assert!(g.arena.as_ref().unwrap().snake(30).is_some());
    }

    #[test]
    fn chat_closed_while_playing() {
        let (mut g, t) = in_round(2);
        assert!(!g.say(1, "x", t));
        win(&mut g, 1, t);
        assert!(!g.chat_open(t));
        let (mut g, t) = game(2);
        assert!(g.say(1, "h\u{1b}é", t));
        assert_eq!(g.chat[0].text, "hé");
        assert!(!g.chat_open(t + secs(8)));
    }
}
