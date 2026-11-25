//! Simulation d'une manche. Déterministe à l'aléa des pommes près, sans E/S ni horloge.

use std::collections::{HashSet, VecDeque};

pub const W: i32 = 76;
pub const H: i32 = 36;
/// Pas de simulation par seconde ; un serpent avance un pas sur deux, à chaque pas s'il fonce.
pub const STEPS_PER_SEC: u64 = 20;
const START_LEN: usize = 5;

/// Accélération : pas gagnés, longueur dépensée, longueur minimale pour s'en servir.
const BOOST_STEPS: u32 = 10;
pub const BOOST_COST: usize = 2;
const BOOST_MIN: usize = START_LEN + BOOST_COST;

/// Rétrécissement : début, rythme, palier de duel, puis fermeture complète.
const SHRINK_FROM: u64 = 25 * STEPS_PER_SEC;
const SHRINK_EVERY: u64 = 4 * STEPS_PER_SEC;
const DUEL_MARGIN: i32 = 11;
const CLOSE_FROM: u64 = 110 * STEPS_PER_SEC;

pub type Cell = (i32, i32);

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

impl Dir {
    fn delta(self) -> Cell {
        match self {
            Dir::Up => (0, -1),
            Dir::Down => (0, 1),
            Dir::Left => (-1, 0),
            Dir::Right => (1, 0),
        }
    }

    fn opposite(self, other: Dir) -> bool {
        let (a, b) = (self.delta(), other.delta());
        a.0 + b.0 == 0 && a.1 + b.1 == 0
    }
}

#[derive(Clone)]
pub struct Snake {
    pub id: u64,
    pub name: String,
    pub color: usize,
    /// Tête en premier. Vide une fois mort.
    pub body: VecDeque<Cell>,
    pub alive: bool,
    pub kills: u32,
    pub longest: usize,
    /// A touché une commande pendant la manche.
    pub active: bool,
    pub killer: Option<String>,
    pub boost: u32,
    dir: Dir,
    turns: VecDeque<Dir>,
    grow: usize,
}

#[derive(Clone)]
pub struct Arena {
    pub snakes: Vec<Snake>,
    pub food: Vec<Cell>,
    /// Épaisseur du mur sur chaque bord.
    pub margin: i32,
    pub step: u64,
    solo: bool,
}

impl Arena {
    /// `players` : identifiant, nom, couleur.
    pub fn new(players: &[(u64, String, usize)]) -> Self {
        let n = players.len().max(1) as f64;
        let turn = rand::random_range(0.0..std::f64::consts::TAU);
        let snakes = players
            .iter()
            .enumerate()
            .map(|(i, (id, name, color))| {
                let a = turn + i as f64 / n * std::f64::consts::TAU;
                let x = (W as f64 / 2.0 + a.cos() * 27.0) as i32;
                let y = (H as f64 / 2.0 + a.sin() * 11.0) as i32;
                // Vers le centre, la queue vers l'extérieur.
                let dir = match a.cos().abs() > a.sin().abs() {
                    true if a.cos() > 0.0 => Dir::Left,
                    true => Dir::Right,
                    false if a.sin() > 0.0 => Dir::Up,
                    false => Dir::Down,
                };
                let d = dir.delta();
                let body = (0..START_LEN as i32).map(|k| (x - d.0 * k, y - d.1 * k)).collect();
                Snake {
                    id: *id,
                    name: name.clone(),
                    color: *color,
                    body,
                    alive: true,
                    kills: 0,
                    longest: START_LEN,
                    active: false,
                    killer: None,
                    boost: 0,
                    dir,
                    turns: VecDeque::new(),
                    grow: 0,
                }
            })
            .collect();
        let mut arena = Arena { snakes, food: Vec::new(), margin: 0, step: 0, solo: players.len() < 2 };
        arena.feed();
        arena
    }

    pub fn snake(&self, id: u64) -> Option<&Snake> {
        self.snakes.iter().find(|s| s.id == id)
    }

    pub fn alive(&self) -> usize {
        self.snakes.iter().filter(|s| s.alive).count()
    }

    pub fn over(&self) -> bool {
        self.alive() <= if self.solo { 0 } else { 1 }
    }

    pub fn winner(&self) -> Option<&Snake> {
        if self.solo { None } else { self.snakes.iter().find(|s| s.alive) }
    }

    pub fn inside(&self, (x, y): Cell) -> bool {
        let m = self.margin;
        (m..W - m).contains(&x) && (m..H - m).contains(&y)
    }

    /// Pas restants avant que le mur n'avance ; absent s'il est à l'arrêt.
    pub fn next_shrink(&self) -> Option<u64> {
        if self.solo || self.margin >= H / 2 {
            return None;
        }
        let from = if self.margin < DUEL_MARGIN { SHRINK_FROM } else { CLOSE_FROM };
        if self.step < from {
            return Some(from - self.step);
        }
        Some(SHRINK_EVERY - (self.step - from) % SHRINK_EVERY)
    }

    pub fn steer(&mut self, id: u64, dir: Dir) {
        let Some(s) = self.snakes.iter_mut().find(|s| s.id == id && s.alive) else { return };
        s.active = true;
        let last = s.turns.back().copied().unwrap_or(s.dir);
        // Deux virages d'avance : compense la latence dans les demi-tours serrés.
        if dir != last && !dir.opposite(last) && s.turns.len() < 2 {
            s.turns.push_back(dir);
        }
    }

    pub fn boost(&mut self, id: u64) {
        let Some(s) = self.snakes.iter_mut().find(|s| s.id == id && s.alive) else { return };
        s.active = true;
        if s.boost == 0 && s.body.len() >= BOOST_MIN {
            s.boost = BOOST_STEPS;
            s.body.truncate(s.body.len() - BOOST_COST);
        }
    }

    pub fn leave(&mut self, id: u64) {
        if let Some(i) = self.snakes.iter().position(|s| s.id == id) {
            if self.snakes[i].alive {
                self.kill(i);
            }
            self.snakes.remove(i);
        }
    }

    pub fn advance(&mut self) {
        self.step += 1;
        if self.next_shrink() == Some(SHRINK_EVERY) {
            self.shrink();
        }
        let normal = self.step.is_multiple_of(2);
        let mut eaten = HashSet::new();
        for s in self.snakes.iter_mut().filter(|s| s.alive) {
            let fast = s.boost > 0;
            s.boost = s.boost.saturating_sub(1);
            if !normal && !fast {
                continue;
            }
            if let Some(d) = s.turns.pop_front() {
                s.dir = d;
            }
            let (head, d) = (s.body[0], s.dir.delta());
            let head = (head.0 + d.0, head.1 + d.1);
            s.body.push_front(head);
            if self.food.contains(&head) {
                eaten.insert(head);
                s.grow += 1;
            }
            if s.grow > 0 {
                s.grow -= 1;
            } else {
                s.body.pop_back();
            }
            s.longest = s.longest.max(s.body.len());
        }
        self.food.retain(|c| !eaten.contains(c));

        // Tous les chocs sont jugés sur la même image, puis appliqués ensemble.
        let crashes: Vec<(usize, Option<usize>)> =
            (0..self.snakes.len()).filter_map(|i| self.crash(i).map(|by| (i, by))).collect();
        for (i, by) in crashes {
            if let Some(k) = by {
                self.snakes[k].kills += 1;
                self.snakes[i].killer = Some(self.snakes[k].name.clone());
            }
            self.kill(i);
        }
        self.feed();
    }

    /// Choc du serpent `i` : `Some(tueur)` s'il meurt.
    fn crash(&self, i: usize) -> Option<Option<usize>> {
        let s = &self.snakes[i];
        if !s.alive {
            return None;
        }
        let head = s.body[0];
        if !self.inside(head) {
            return Some(None);
        }
        for (j, o) in self.snakes.iter().enumerate().filter(|(_, o)| o.alive) {
            for (k, cell) in o.body.iter().enumerate() {
                if *cell != head || (i == j && k == 0) {
                    continue;
                }
                if k > 0 {
                    return Some((i != j).then_some(j));
                }
                // Tête contre tête : le plus long survit.
                match s.body.len().cmp(&o.body.len()) {
                    std::cmp::Ordering::Greater => {}
                    std::cmp::Ordering::Less => return Some(Some(j)),
                    std::cmp::Ordering::Equal => return Some(None),
                }
            }
        }
        None
    }

    /// Le corps devient des pommes, une case sur deux.
    fn kill(&mut self, i: usize) {
        let s = &mut self.snakes[i];
        s.alive = false;
        let body = std::mem::take(&mut s.body);
        for c in body.into_iter().step_by(2) {
            if self.inside(c) && !self.food.contains(&c) {
                self.food.push(c);
            }
        }
    }

    fn shrink(&mut self) {
        self.margin += 1;
        let m = self.margin;
        let inside = |c: &Cell| (m..W - m).contains(&c.0) && (m..H - m).contains(&c.1);
        self.food.retain(inside);
        // Le mur coupe la queue ; une tête dehors meurt au contrôle des chocs.
        for s in self.snakes.iter_mut().filter(|s| s.alive) {
            if let Some(cut) = s.body.iter().skip(1).position(|c| !inside(c)) {
                s.body.truncate((cut + 1).max(2));
            }
        }
    }

    fn feed(&mut self) {
        let free = (W - 2 * self.margin) * (H - 2 * self.margin);
        if free <= 0 {
            return;
        }
        let wanted = (6 + 2 * self.snakes.len()).min(free as usize / 12);
        for _ in 0..20 {
            if self.food.len() >= wanted {
                break;
            }
            let m = self.margin;
            let c = (rand::random_range(m..W - m), rand::random_range(m..H - m));
            let taken = self.food.contains(&c) || self.snakes.iter().any(|s| s.body.contains(&c));
            if !taken {
                self.food.push(c);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arena(n: u64) -> Arena {
        let p: Vec<_> = (1..=n).map(|i| (i, format!("s{i}"), 0)).collect();
        let mut a = Arena::new(&p);
        a.food.clear();
        a
    }

    fn place(a: &mut Arena, i: usize, cells: &[Cell], dir: Dir) {
        a.snakes[i].body = cells.iter().copied().collect();
        a.snakes[i].dir = dir;
    }

    /// Avance jusqu'au prochain déplacement normal.
    fn go(a: &mut Arena) {
        a.advance();
        if !a.step.is_multiple_of(2) {
            a.advance();
        }
    }

    #[test]
    fn spawns_are_inside_and_disjoint() {
        for n in 1..=24 {
            let a = arena(n);
            for (i, s) in a.snakes.iter().enumerate() {
                assert!(s.body.iter().all(|c| a.inside(*c)));
                for o in &a.snakes[i + 1..] {
                    assert!(s.body.iter().all(|c| !o.body.contains(c)), "{n}");
                }
            }
        }
    }

    #[test]
    fn eats_and_grows() {
        let mut a = arena(1);
        place(&mut a, 0, &[(10, 10), (9, 10)], Dir::Right);
        a.food = vec![(11, 10)];
        go(&mut a);
        assert_eq!(a.snakes[0].body.len(), 3);
        assert!(!a.food.contains(&(11, 10)));
    }

    #[test]
    fn no_u_turn_but_two_queued_turns() {
        let mut a = arena(1);
        place(&mut a, 0, &[(10, 10), (9, 10), (8, 10)], Dir::Right);
        a.steer(1, Dir::Left);
        a.steer(1, Dir::Up);
        a.steer(1, Dir::Left);
        go(&mut a);
        go(&mut a);
        assert_eq!(a.snakes[0].body[0], (9, 9));
        assert!(a.snakes[0].alive);
    }

    #[test]
    fn body_crash_credits_killer_and_leaves_food() {
        let mut a = arena(2);
        place(&mut a, 0, &[(10, 10), (9, 10), (8, 10)], Dir::Right);
        place(&mut a, 1, &[(11, 8), (11, 9), (11, 10), (11, 11), (11, 12)], Dir::Up);
        go(&mut a);
        assert!(!a.snakes[0].alive && a.snakes[1].alive);
        assert_eq!((a.snakes[1].kills, a.snakes[0].killer.as_deref()), (1, Some("s2")));
        assert!(a.food.contains(&(9, 10)));
        assert!(a.over());
        assert_eq!(a.winner().unwrap().id, 2);
    }

    #[test]
    fn head_to_head_longest_survives_equal_both_die() {
        let mut a = arena(2);
        place(&mut a, 0, &[(10, 10), (9, 10), (8, 10)], Dir::Right);
        place(&mut a, 1, &[(12, 10), (13, 10)], Dir::Left);
        go(&mut a);
        assert!(a.snakes[0].alive && !a.snakes[1].alive);

        let mut a = arena(2);
        place(&mut a, 0, &[(10, 10), (9, 10)], Dir::Right);
        place(&mut a, 1, &[(12, 10), (13, 10)], Dir::Left);
        go(&mut a);
        assert_eq!(a.alive(), 0);
        assert!(a.over() && a.winner().is_none());
    }

    #[test]
    fn wall_kills_without_killer() {
        let mut a = arena(2);
        place(&mut a, 0, &[(0, 5), (1, 5)], Dir::Left);
        go(&mut a);
        assert!(!a.snakes[0].alive && a.snakes[0].killer.is_none());
    }

    #[test]
    fn boost_is_twice_as_fast_and_costs_length() {
        let mut a = arena(1);
        let body: Vec<Cell> = (0..8).map(|k| (20 - k, 10)).collect();
        place(&mut a, 0, &body, Dir::Right);
        a.boost(1);
        assert_eq!(a.snakes[0].body.len(), 8 - BOOST_COST);
        for _ in 0..BOOST_STEPS {
            a.food.clear();
            a.advance();
        }
        assert_eq!(a.snakes[0].body[0].0, 20 + BOOST_STEPS as i32);
        a.boost(1);
        assert_eq!(a.snakes[0].body.len(), 6);
    }

    #[test]
    fn walls_close_until_the_round_ends() {
        let mut a = arena(2);
        // Deux serpents sans fin : seul le mur peut les tuer.
        while !a.over() {
            for i in 0..2 {
                if a.snakes[i].alive && a.margin < H / 2 - 1 {
                    let y = H / 2 - 1 + i as i32;
                    place(&mut a, i, &[(W / 2, y), (W / 2 - 1, y)], Dir::Right);
                }
            }
            a.advance();
            assert!(a.step < 200 * STEPS_PER_SEC);
        }
        assert!(a.step > CLOSE_FROM);
    }

    #[test]
    fn solo_arena_never_shrinks() {
        let mut a = arena(1);
        for _ in 0..3000 {
            place(&mut a, 0, &[(W / 2, H / 2), (W / 2 - 1, H / 2)], Dir::Right);
            a.advance();
        }
        assert_eq!((a.margin, a.over()), (0, false));
    }

    #[test]
    fn leaving_ends_round() {
        let mut a = arena(2);
        a.leave(1);
        assert!(a.over());
        assert_eq!(a.winner().unwrap().id, 2);
    }
}
