//! Rendu. Palette 256 couleurs : COLORTERM ne traverse pas SSH.

use std::time::Instant;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

use crate::accounts::Stats;
use crate::arena::{Arena, H, STEPS_PER_SEC, Snake, W};
use crate::game::{COLORS, Game, Phase, Player, ROUNDS_TO_WIN};

const ACCENT: Color = Color::Indexed(114);
const DIM: Color = Color::Indexed(245);
const HAIRLINE: Color = Color::Indexed(239);
const KEY_BG: Color = Color::Indexed(238);
const TEXT: Color = Color::Indexed(252);
const WHITE: Color = Color::Indexed(231);
const GOOD: Color = Color::Indexed(114);
const BAD: Color = Color::Indexed(203);
const GOLD: Color = Color::Indexed(222);

const FLOOR: Color = Color::Indexed(234);
const WALL: Color = Color::Indexed(52);
/// Bord sur le point de tomber.
const WALL_NEXT: Color = Color::Indexed(88);
const FOOD: Color = Color::Indexed(229);

/// Une couleur par personne, serpent et chat. Pas de rouge : c'est le mur.
const PALETTE: [u8; COLORS] =
    [114, 81, 213, 215, 141, 43, 227, 75, 208, 155, 175, 123, 180, 99, 151, 183];

pub const MIN_W: u16 = 80;
pub const MIN_H: u16 = 24;

const LOGO: [&str; 3] = [
    "█▀▀ █▀▀ █ █   █▀▀ █▄ █ █▀█ █▄▀ █▀▀",
    "▀▀█ ▀▀█ █▀█   ▀▀█ █ ▀█ █▀█ █▀▄ █▀▀",
    "▀▀▀ ▀▀▀ ▀ ▀   ▀▀▀ ▀  ▀ ▀ ▀ ▀ ▀ ▀▀▀",
];

pub enum View<'a> {
    Naming(&'a str, &'a str),
    Game(&'a Game, u64, &'a str, Option<&'a Panel>),
}

/// Fiche de stats ; `stats` vide si le compte n'existe pas.
pub struct Panel {
    pub name: String,
    pub stats: Option<Stats>,
}

fn fg(c: Color) -> Style {
    Style::new().fg(c)
}

fn bold(c: Color) -> Style {
    fg(c).add_modifier(Modifier::BOLD)
}

fn color(i: usize) -> Color {
    Color::Indexed(PALETTE[i % COLORS])
}

fn put(buf: &mut Buffer, area: Rect, y: u16, line: Line) {
    if y < area.bottom() {
        Paragraph::new(line).render(Rect { y, height: 1, ..area }, buf);
    }
}

fn center(buf: &mut Buffer, area: Rect, y: u16, line: Line) {
    put(buf, area, y, line.centered());
}

fn rule(buf: &mut Buffer, area: Rect, y: u16) {
    put(buf, area, y, Line::styled("─".repeat(area.width as usize), fg(HAIRLINE)));
}

fn secs_left(until: Instant, now: Instant) -> u64 {
    until.saturating_duration_since(now).as_millis().div_ceil(1000) as u64
}

fn footer(buf: &mut Buffer, area: Rect, keys: &[(&str, &str)]) {
    let mut spans = Vec::new();
    for (k, label) in keys.iter().chain(&[("Ctrl+C", "quitter")]) {
        spans.push(Span::styled(format!(" {k} "), bold(TEXT).bg(KEY_BG)));
        spans.push(Span::styled(format!(" {label}  "), fg(DIM)));
    }
    center(buf, area, area.bottom() - 1, Line::from(spans));
}

/// Consigne sur trois lignes pleines, impossible à rater.
fn banner(buf: &mut Buffer, area: Rect, y: u16, text: &str, bg: Color, ink: Color) {
    for row in y..y + 3 {
        // Efface l'arène dessous : ses demi-blocs traverseraient le fond.
        put(buf, area, row, Line::styled(" ".repeat(area.width as usize), Style::new().bg(bg)));
    }
    center(buf, area, y + 1, Line::styled(text.to_string(), bold(ink).bg(bg)));
}

pub fn draw(buf: &mut Buffer, view: View, now: Instant) {
    let full = buf.area;
    if full.width < MIN_W || full.height < MIN_H {
        let msg = format!("agrandis ton terminal ({MIN_W}x{MIN_H} min, là {}x{})", full.width, full.height);
        return center(buf, full, full.height / 2, Line::styled(msg, fg(BAD)));
    }
    let area = Rect { x: (full.width - 78) / 2, y: 1, width: 78, height: full.height - 1 };
    match view {
        View::Naming(name, error) => naming(buf, area, name, error),
        View::Game(_, _, _, Some(p)) => panel(buf, area, p),
        View::Game(g, me, draft, None) => match g.phase {
            Phase::Lobby { start_at } => lobby(buf, area, g, me, draft, start_at, now),
            Phase::Results { until } => results(buf, area, g, me, draft, until, now),
            _ => play(buf, full, g, me, now),
        },
    }
}

fn logo(buf: &mut Buffer, area: Rect) -> u16 {
    for (i, l) in LOGO.iter().enumerate() {
        center(buf, area, area.y + i as u16, Line::styled(*l, fg(ACCENT)));
    }
    area.y + LOGO.len() as u16 + 1
}

fn header(buf: &mut Buffer, area: Rect, title: &str, right: String) -> u16 {
    put(buf, area, area.y, Line::styled(format!("▍{title}"), bold(ACCENT)));
    put(buf, area, area.y, Line::styled(right, bold(DIM)).right_aligned());
    rule(buf, area, area.y + 1);
    area.y + 3
}

fn naming(buf: &mut Buffer, area: Rect, name: &str, error: &str) {
    let y = logo(buf, area);
    center(buf, area, y, Line::styled("mange, piège les autres, reste en vie", fg(DIM)));
    let line = Line::from(vec![
        Span::styled("Ton pseudo  ", fg(DIM)),
        Span::styled(name.to_string(), bold(TEXT)),
        Span::styled("█", fg(ACCENT)),
    ]);
    center(buf, area, y + 3, line);
    center(buf, area, y + 4, Line::styled(error.to_string(), fg(BAD)));
    let hint = "reprends toujours le même pour garder tes stats";
    center(buf, area, y + 6, Line::styled(hint, fg(DIM)));
    footer(buf, area, &[("Entrée", "rejoindre")]);
}

fn panel(buf: &mut Buffer, area: Rect, p: &Panel) {
    let y = header(buf, area, &format!("PROFIL · {}", p.name), String::new());
    let Some(s) = &p.stats else {
        put(buf, area, y, Line::styled("Personne n'a encore joué sous ce pseudo.", fg(TEXT)));
        return footer(buf, area, &[("Entrée", "fermer")]);
    };
    let rows = [
        ("Matchs gagnés", s.match_wins.to_string()),
        ("Manches gagnées", format!("{} sur {}", s.round_wins, s.rounds)),
        ("Serpents piégés", s.kills.to_string()),
        ("Plus grande taille", s.longest.to_string()),
    ];
    for (i, (label, value)) in rows.into_iter().enumerate() {
        let line = Line::from(vec![
            Span::styled(format!("{label:<21}"), fg(DIM)),
            Span::styled(value, bold(TEXT)),
        ]);
        put(buf, area, y + i as u16, line);
    }
    footer(buf, area, &[("Entrée", "fermer")]);
}

/// Fenêtre de `max` lignes contenant toujours le joueur courant.
fn window<'a>(list: &[&'a Player], me: u64, max: usize) -> Vec<(usize, &'a Player)> {
    let mine = list.iter().position(|p| p.id == me).unwrap_or(0);
    let skip = (mine + 1).saturating_sub(max);
    list.iter().copied().enumerate().skip(skip).take(max).collect()
}

fn lobby(buf: &mut Buffer, area: Rect, g: &Game, me: u64, draft: &str, start: Option<Instant>, now: Instant) {
    let mut y = logo(buf, area) - 1;
    let rules = Line::from(vec![
        Span::styled("◆ mange", bold(FOOD)),
        Span::styled("  ·  ", fg(HAIRLINE)),
        Span::styled("mur ou corps : mort", bold(BAD)),
        Span::styled("  ·  ", fg(HAIRLINE)),
        Span::styled(format!("dernier en vie : 1 point ({ROUNDS_TO_WIN} = gagné)"), bold(GOOD)),
    ]);
    center(buf, area, y, rules);
    y += 1;
    let status = match start {
        Some(t) => Line::styled(format!("Départ dans {}", secs_left(t, now)), bold(GOOD)),
        None => Line::styled("En attente d'un adversaire...", fg(DIM)),
    };
    center(buf, area, y, status);
    rule(buf, area, y + 1);
    y += 2;

    let half = area.width / 2;
    let left = Rect { width: half - 1, ..area };
    let right = Rect { x: area.x + half + 1, width: area.width - half - 1, ..area };
    let title = format!("Joueurs ({} prêts sur {})", g.ready_count(), g.players.len());
    put(buf, left, y, Line::styled(title, bold(TEXT)));
    put(buf, right, y, Line::styled("Classement (matchs, manches)", bold(TEXT)));
    y += 1;

    let all: Vec<&Player> = g.players.iter().collect();
    let avail = (area.bottom() - 2).saturating_sub(y) as usize;
    let rows = all.len().max(g.top.len()).max(1).min(avail.saturating_sub(5));
    for (i, (_, p)) in window(&all, me, rows).into_iter().enumerate() {
        let mark = if p.ready { "◆ " } else { "◇ " };
        let mut spans = vec![Span::styled(mark, fg(color(p.color))), Span::styled(p.name.clone(), bold(color(p.color)))];
        if p.id == me {
            spans.push(Span::styled("  (toi)", fg(DIM)));
        }
        put(buf, left, y + i as u16, Line::from(spans));
    }
    if g.top.is_empty() {
        put(buf, right, y, Line::styled("personne pour l'instant", fg(DIM)));
    }
    for (i, a) in g.top.iter().enumerate().take(rows) {
        let line = Line::from(vec![
            Span::styled(format!("{}. ", i + 1), fg(DIM)),
            Span::styled(format!("{:<17}", a.name), fg(TEXT)),
            Span::styled(format!("{:>3}", a.stats.match_wins), bold(GOLD)),
            Span::styled(format!("{:>5}", a.stats.round_wins), fg(DIM)),
        ]);
        put(buf, right, y + i as u16, line);
    }
    chat(buf, area, y + rows as u16, g, draft, now);
    let alone = ready_alone(g, me) && start.is_none();
    let label = match g.player(me).is_some_and(|p| p.ready) {
        _ if !draft.trim().is_empty() => "envoyer",
        _ if alone => "jouer seul",
        true => "pas prêt",
        false => "prêt",
    };
    footer(buf, area, &[("Entrée", label), ("Ctrl+P", "profil")]);
}

fn ready_alone(g: &Game, me: u64) -> bool {
    g.ready_count() == 1 && g.player(me).is_some_and(|p| p.ready)
}

fn chat(buf: &mut Buffer, area: Rect, y: u16, g: &Game, draft: &str, now: Instant) {
    rule(buf, area, y);
    let input = area.bottom() - 2;
    let width = area.width as usize;
    let mut lines = Vec::new();
    for m in &g.chat {
        let head = m.name.len() + 2;
        let body: Vec<char> = m.text.chars().collect();
        for (i, (s, e)) in wrap(&body, width - head).into_iter().enumerate() {
            let name = if i == 0 { format!("{}: ", m.name) } else { " ".repeat(head) };
            lines.push(Line::from(vec![
                Span::styled(name, bold(color(m.color))),
                Span::styled(body[s..e].iter().collect::<String>(), fg(TEXT)),
            ]));
        }
    }
    let rows = input.saturating_sub(y + 1) as usize;
    let skip = lines.len().saturating_sub(rows);
    for (i, l) in lines.into_iter().skip(skip).enumerate() {
        put(buf, area, y + 1 + i as u16, l);
    }
    let line = if !g.chat_open(now) {
        Line::styled("> chat fermé : mains sur les flèches", fg(DIM))
    } else if draft.is_empty() {
        Line::from(vec![Span::styled("> █", fg(ACCENT)), Span::styled(" écris un message", fg(DIM))])
    } else {
        let n = draft.chars().count();
        let tail: String = draft.chars().skip(n.saturating_sub(width - 3)).collect();
        Line::from(vec![
            Span::styled("> ", fg(ACCENT)),
            Span::styled(tail, fg(TEXT)),
            Span::styled("█", fg(ACCENT)),
        ])
    };
    put(buf, area, input, line);
}

/// Coupe le texte sur les espaces ; renvoie des intervalles [début, fin).
fn wrap(b: &[char], width: usize) -> Vec<(usize, usize)> {
    let (mut out, mut start) = (Vec::new(), 0);
    while b.len() - start > width {
        let cut = (start + 1..start + width).rev().find(|&i| b[i] == ' ').map_or(start + width, |i| i + 1);
        out.push((start, cut));
        start = cut;
    }
    out.push((start, b.len()));
    out
}

fn points(p: &Player) -> Span<'static> {
    let full = p.points.min(ROUNDS_TO_WIN) as usize;
    let dots = format!("{}{}", "●".repeat(full), "○".repeat(ROUNDS_TO_WIN as usize - full));
    Span::styled(dots, fg(GOLD))
}

/// Arène plein écran : deux cases par caractère, grâce au demi-bloc.
fn play(buf: &mut Buffer, full: Rect, g: &Game, me: u64, now: Instant) {
    let Some(a) = &g.arena else { return };
    let frame_h = (H / 2) as u16 + 2;
    let area = Rect {
        x: (full.width - W as u16 - 2) / 2,
        y: (full.height - frame_h - 3) / 2,
        width: W as u16 + 2,
        height: frame_h + 3,
    };
    let mine = a.snake(me);

    // Ligne du haut : moi, manche, mur.
    let left = match mine {
        Some(s) if s.alive => Line::from(vec![
            Span::styled("TOI ██ ", bold(color(s.color))),
            Span::styled(format!("taille {} ", s.body.len()), bold(TEXT)),
            Span::styled(format!(" piégés {} ", s.kills), fg(DIM)),
            Span::styled(if s.boost > 0 { " »» FONCE" } else { "" }, bold(GOLD)),
        ]),
        Some(s) => {
            let by = s.killer.as_ref().map_or("le mur ou toi-même".into(), |k| k.clone());
            Line::styled(format!("MORT, piégé par {by}"), bold(BAD))
        }
        None if g.waiting(me) => Line::styled("TU ENTRES À LA PROCHAINE MANCHE", bold(GOLD)),
        None => Line::styled("SPECTATEUR", bold(DIM)),
    };
    put(buf, area, area.y, left);
    let right = match a.next_shrink() {
        Some(n) => {
            let secs = n.div_ceil(STEPS_PER_SEC);
            let c = if secs <= 3 { BAD } else { DIM };
            Line::styled(format!("le mur avance dans {secs}s"), bold(c))
        }
        None => Line::styled("entraînement", fg(DIM)),
    };
    put(buf, area, area.y, right.right_aligned());

    // Cadre et cases.
    let field = Rect { x: area.x + 1, y: area.y + 2, width: W as u16, height: (H / 2) as u16 };
    let danger = matches!(a.next_shrink(), Some(n) if n <= 3 * STEPS_PER_SEC);
    let border = fg(if danger { WALL_NEXT } else { HAIRLINE });
    let bar = "─".repeat(W as usize);
    put(buf, area, area.y + 1, Line::styled(format!("╭{bar}╮"), border));
    put(buf, area, field.bottom(), Line::styled(format!("╰{bar}╯"), border));
    let grid = pixels(a, me, danger);
    for row in 0..field.height {
        let y = field.y + row;
        buf[(area.x, y)].set_symbol("│").set_style(border);
        buf[(field.right(), y)].set_symbol("│").set_style(border);
        for col in 0..field.width {
            let at = |dy: u16| grid[(row * 2 + dy) as usize * W as usize + col as usize];
            let (top, bottom) = (at(0), at(1));
            // Même caractère partout : seules les couleurs changent, l'image reste légère.
            buf[(field.x + col, y)].set_symbol("▀").set_style(Style::new().fg(top).bg(bottom));
        }
    }

    // Scores.
    let mut spans = vec![Span::styled(format!("Manche {}  ", g.round), bold(TEXT))];
    for p in g.scoreboard() {
        let alive = a.snake(p.id).is_some_and(|s| s.alive);
        let style = if alive { bold(color(p.color)) } else { fg(HAIRLINE) };
        spans.push(Span::styled(p.name.clone(), style));
        spans.push(Span::raw(" "));
        spans.push(points(p));
        spans.push(Span::raw("  "));
    }
    put(buf, area, field.bottom() + 1, Line::from(spans));
    let mut keys = match mine {
        Some(_) => vec![("← ↑ ↓ →", "tourner"), ("Espace", "foncer (-2)")],
        None if g.waiting(me) => vec![("Entrée", "rester spectateur")],
        None => vec![("Entrée", "entrer à la prochaine manche")],
    };
    if g.alone() {
        keys.push(("Tab", "relancer"));
    }
    let foot = Rect { height: field.bottom() + 3 - area.y, ..area }.intersection(full);
    footer(buf, foot, &keys);

    // Bandeau par-dessus l'arène.
    let mid = Rect { x: field.x + 14, width: field.width - 28, ..field };
    match g.phase {
        Phase::Ready { until } => {
            let left = secs_left(until, now);
            match mine {
                Some(s) => {
                    let text = format!("TU ES CETTE COULEUR   ·   {left}");
                    banner(buf, mid, field.y + 7, &text, color(s.color), FLOOR);
                }
                None => banner(buf, mid, field.y + 7, &format!("départ dans {left}"), KEY_BG, TEXT),
            }
        }
        Phase::RoundEnd { until } => {
            let (text, bg) = match &g.round_winner {
                Some(n) => {
                    let c = g.players.iter().find(|p| p.name == *n).map_or(GOLD, |p| color(p.color));
                    (format!("{n} gagne la manche !   suite dans {}", secs_left(until, now)), c)
                }
                None => (format!("manche nulle   suite dans {}", secs_left(until, now)), KEY_BG),
            };
            let ink = if bg == KEY_BG { TEXT } else { FLOOR };
            banner(buf, mid, field.y + 7, &text, bg, ink);
        }
        _ => {}
    }
}

fn pixels(a: &Arena, me: u64, danger: bool) -> Vec<Color> {
    let mut grid = vec![FLOOR; (W * H) as usize];
    let mut set = |(x, y): (i32, i32), c: Color| {
        if (0..W).contains(&x) && (0..H).contains(&y) {
            grid[(y * W + x) as usize] = c;
        }
    };
    let m = a.margin;
    for y in 0..H {
        for x in 0..W {
            if !a.inside((x, y)) {
                set((x, y), WALL);
            } else if danger && (x == m || y == m || x == W - 1 - m || y == H - 1 - m) {
                set((x, y), WALL_NEXT);
            }
        }
    }
    for c in &a.food {
        set(*c, FOOD);
    }
    for s in a.snakes.iter().filter(|s| s.alive) {
        draw_snake(&mut set, s, s.id == me);
    }
    grid
}

fn draw_snake(set: &mut impl FnMut((i32, i32), Color), s: &Snake, mine: bool) {
    for c in s.body.iter().skip(1) {
        set(*c, color(s.color));
    }
    // Tête blanche pour soi, claire pour les autres.
    let head = if mine { WHITE } else { Color::Indexed(250) };
    set(s.body[0], head);
}

fn results(buf: &mut Buffer, area: Rect, g: &Game, me: u64, draft: &str, until: Instant, now: Instant) {
    let right = format!("salon dans {}", secs_left(until, now));
    let y = header(buf, area, "FIN DU MATCH", right) - 1;
    let solo = g.arena.as_ref().is_some_and(|a| a.snakes.len() < 2) && g.round == 1;
    match &g.champion {
        Some(n) => {
            let c = g.players.iter().find(|p| p.name == *n).map_or(GOLD, |p| color(p.color));
            banner(buf, area, y, &format!("★  {n} remporte le match !  ★"), c, FLOOR);
        }
        None if solo => {
            let best = g.arena.as_ref().and_then(|a| a.snake(me)).map_or(0, |s| s.longest);
            banner(buf, area, y, &format!("taille {best}"), ACCENT, FLOOR);
        }
        None => banner(buf, area, y, "match nul", KEY_BG, TEXT),
    }
    let mut y = y + 3;
    let head = format!("{:>2} {:<17}{:>8}{:>18}{:>10}", "#", "joueur", "manches", "piégés (manche)", "taille");
    put(buf, area, y, Line::styled(head, fg(DIM)));
    y += 1;
    let board = g.scoreboard();
    let avail = (area.bottom() - 2).saturating_sub(y) as usize;
    let rows = board.len().min(avail.saturating_sub(5));
    for (i, (rank, p)) in window(&board, me, rows).into_iter().enumerate() {
        let s = g.arena.as_ref().and_then(|a| a.snake(p.id));
        let line = Line::from(vec![
            Span::styled(format!("{:>2} ", rank + 1), fg(DIM)),
            Span::styled(format!("{:<17}", p.name), bold(color(p.color))),
            Span::styled(format!("{:>8}", p.points), bold(GOLD)),
            Span::styled(format!("{:>18}", s.map_or(0, |s| s.kills)), fg(TEXT)),
            Span::styled(format!("{:>10}", s.map_or(0, |s| s.longest)), fg(TEXT)),
        ]);
        put(buf, area, y + i as u16, line);
    }
    chat(buf, area, y + rows as u16, g, draft, now);
    let mut keys = vec![("Entrée", "envoyer")];
    if g.alone() {
        keys.insert(0, ("Tab", "relancer"));
    }
    footer(buf, area, &keys);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arena::Dir;
    use std::time::Duration;

    #[test]
    fn draws_every_phase_at_any_size() {
        for n in [1, 2, 30] {
            let now = Instant::now();
            let mut g = Game::new();
            for id in 0..n {
                g.join(id, "abcdefghijklmnop", Stats::default(), now);
            }
            g.say(0, &"é".repeat(120), now);
            g.press_enter(0, now);
            let mut t = now;
            for _ in 0..400 {
                t += Duration::from_millis(500);
                g.tick(t);
                g.steer(0, Dir::Up);
                g.boost(0);
                for (w, h) in [(1, 1), (80, 24), (81, 25), (300, 100)] {
                    for me in [0, 99] {
                        let mut buf = Buffer::empty(Rect::new(0, 0, w, h));
                        draw(&mut buf, View::Game(&g, me, "é", None), t);
                    }
                }
            }
            let mut buf = Buffer::empty(Rect::new(0, 0, 80, 24));
            let p = Panel { name: "x".into(), stats: Some(Stats::default()) };
            draw(&mut buf, View::Game(&g, 0, "", Some(&p)), t);
            draw(&mut buf, View::Naming("x", "erreur"), t);
        }
    }
}
