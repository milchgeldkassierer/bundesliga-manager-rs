//! The team editor's strength recomputation, shared with the Mod 2026 generator.
//!
//! Origin: TEAMED.EXE

use bmp_formats::mana::{self, ManaDat};

use crate::rng::{Rng, clamp_int, ldiv};

/// One league group of the strength recomputation.
pub struct Group {
    /// First club after the group; the last group ends at [`mana::GERMAN_CLUB_COUNT`].
    pub end: usize,
    /// Base strength.
    pub base: i8,
    /// Clubs per league; the rank term is `size - rank`.
    pub size: i8,
    /// Whether all clubs of the group get random strengths (the original's amateurs).
    pub random: bool,
}

/// The league groups of the strength recomputation, in slot order.
pub struct Grouping(pub &'static [Group]);

impl Grouping {
    /// Group of German club `i`.
    pub fn group(&self, i: usize) -> usize {
        self.0
            .iter()
            .position(|g| i < g.end)
            .expect("German club index")
    }
}

/// The original grouping: Bundesliga, 2. Bundesliga, amateurs, i.e. group
/// `(i > 37) + (i > 17)`, with the bases of `DS:0554` and the sizes of `DS:033C`.
pub const ORIGINAL: Grouping = Grouping(&[
    Group {
        end: 18,
        base: 70,
        size: 18,
        random: false,
    },
    Group {
        end: 38,
        base: 34,
        size: 20,
        random: false,
    },
    Group {
        end: 64,
        base: 20,
        size: 20,
        random: true,
    },
]);

// MOD2026: the leagues 0..17 / 18..35 / 36..55 and the pool 56..63, all table-based
// (`docs/mod2026/CHANGES.md`, "Strength grouping"). The 3. Liga keeps the original third
// group's base and size; the pool shares them, as its ranks (3. Liga or Regionalliga) never
// exceed 20.
/// The Mod 2026 grouping of the generator.
pub const MOD2026: Grouping = Grouping(&[
    Group {
        end: 18,
        base: 70,
        size: 18,
        random: false,
    },
    Group {
        end: 36,
        base: 34,
        size: 18,
        random: false,
    },
    Group {
        end: 56,
        base: 20,
        size: 20,
        random: false,
    },
    Group {
        end: 64,
        base: 20,
        size: 20,
        random: false,
    },
]);

/// [`recompute_strengths_grouped`] with the original grouping.
///
/// Origin: TEAMED.EXE 1579:006d (teamed_save_mana, strength recomputation)
pub fn recompute_strengths(m: &mut ManaDat, rng: &mut Rng) {
    recompute_strengths_grouped(m, rng, &ORIGINAL);
}

/// The strength bytes of clubs 0..63 from the points, goals and rank of the previous
/// season (and `random_range`), see `docs/re/teamed/uefa.md` "Strength recomputation".
///
/// Per group the averages of the plus points, goals scored and goals conceded over all
/// 64 clubs of the group are divided by the number of clubs with a rank (0 stays the
/// sum, an average of 0 becomes 1). Clubs without a rank and those of the random group
/// get `base + rand(0..5) - rand(0..5) + rand(0..25)` three times (one `rand(0..25)`
/// shared); the others get values from the rank, the points and the goals. Group 1 (the
/// 2. Bundesliga) is then scaled by 125/100 and clamped to 38..74; all values are clamped to 20..95. The
/// foreign clubs 64..199 keep their bytes.
///
/// Origin: TEAMED.EXE 1579:006d (teamed_save_mana, strength recomputation)
pub fn recompute_strengths_grouped(m: &mut ManaDat, rng: &mut Rng, grouping: &Grouping) {
    let groups = grouping.0.len();
    let mut ranked = vec![0i16; groups];
    let mut points = vec![0i16; groups];
    let mut scored = vec![0i16; groups];
    let mut conceded = vec![0i16; groups];
    for i in 0..mana::GERMAN_CLUB_COUNT {
        let g = grouping.group(i);
        points[g] = points[g].wrapping_add(i16::from(m.points[i][0]));
        scored[g] = scored[g].wrapping_add(i16::from(m.goals[i][0]));
        conceded[g] = conceded[g].wrapping_add(i16::from(m.goals[i][1]));
        if m.rank[i] != 0 {
            ranked[g] += 1;
        }
    }
    for g in 0..groups {
        for sum in [&mut points[g], &mut scored[g], &mut conceded[g]] {
            if ranked[g] != 0 {
                *sum /= ranked[g];
            }
            if *sum == 0 {
                *sum = 1;
            }
        }
    }
    for i in 0..mana::GERMAN_CLUB_COUNT {
        let g = grouping.group(i);
        let group = &grouping.0[g];
        let base = i16::from(group.base);
        // The three bytes in file order ([BP-0x68], [BP-0x66], [BP-0x7A]).
        let (mut first, mut second, mut third);
        if m.rank[i] == 0 || group.random {
            let shared = rng.random_range(0, 0x19);
            let mut value = || {
                let up = rng.random_range(0, 5);
                let down = rng.random_range(0, 5);
                up.wrapping_add(base.wrapping_sub(down))
                    .wrapping_add(shared)
            };
            first = value();
            second = value();
            third = value();
        } else {
            let [goals_for, goals_against] = m.goals[i];
            let has_goals = goals_for != 0 && goals_against != 0;
            let mut table = 0i16;
            if m.points[i][0] != 0 && m.points[i][1] != 0 {
                table = (i16::from(m.points[i][0]) * -100) / points[g] + 100;
            }
            let form = i16::from(group.size) - i16::from(m.rank[i]) - table / 20;
            let mut defence = 0i16;
            if has_goals {
                defence = (i16::from(goals_against) * -100) / conceded[g] + 100;
            }
            first = base + defence / 20 + form;
            first += first / 20;
            let mut attack = 0i16;
            if has_goals {
                attack = ldiv(i32::from(goals_for) * -100, i32::from(scored[g])) as i16 + 100;
            }
            third = base - attack / 20 + form;
            third += third / 20;
            second = base + form;
            second += second / 20;
        }
        if g == 1 {
            for v in [&mut first, &mut second, &mut third] {
                *v = ldiv(i32::from(*v) * 0x7d, 100) as i16;
            }
            for v in [&mut first, &mut third, &mut second] {
                clamp_int(v, 0x26, 0x4a);
            }
        }
        for v in [&mut first, &mut third, &mut second] {
            clamp_int(v, 0x14, 0x5f);
        }
        m.strength[i] = [first as u8, second as u8, third as u8];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups() {
        assert_eq!(ORIGINAL.group(17), 0);
        assert_eq!(ORIGINAL.group(18), 1);
        assert_eq!(ORIGINAL.group(37), 1);
        assert_eq!(ORIGINAL.group(38), 2);
        assert_eq!(ORIGINAL.group(63), 2);
    }

    #[test]
    fn mod_grouping_follows_the_mod_slot_table() {
        let groups = [17, 18, 35, 36, 55, 56, 63].map(|i| MOD2026.group(i));
        assert_eq!(groups, [0, 1, 1, 2, 2, 3, 3]);
    }

    /// The shipped `MANA.DAT` was last written by the editor: the computed (not random)
    /// strengths of the ranked league clubs must come out unchanged.
    #[test]
    fn shipped_strengths_reproduce() {
        let Some(dir) = bmp_formats::data_dir() else {
            return;
        };
        let bytes = std::fs::read(dir.join("MANA.DAT")).unwrap();
        let shipped = ManaDat::parse(&bytes).unwrap();
        let mut m = shipped.clone();
        recompute_strengths(&mut m, &mut Rng::default());
        let mut checked = 0;
        for i in 0..mana::GERMAN_CLUB_COUNT {
            if shipped.rank[i] != 0 && ORIGINAL.group(i) != 2 {
                assert_eq!(m.strength[i], shipped.strength[i], "club {i}");
                checked += 1;
            }
        }
        assert!(checked > 30);
        // The foreign clubs are written back unchanged.
        assert_eq!(m.strength[64..], shipped.strength[64..]);
    }

    #[test]
    fn random_clubs_use_seven_calls_from_the_msc_start_state() {
        let mut m = ManaDat::parse(&vec![0u8; mana::FILE_SIZE]).unwrap();
        let mut rng = Rng::default();
        recompute_strengths(&mut m, &mut rng);
        // All clubs are unranked: 64 x 7 random_range calls.
        let mut expect = Rng::default();
        for _ in 0..64 * 7 {
            expect.rand();
        }
        assert_eq!(rng, expect);
        let mut r = Rng::default();
        let shared = r.random_range(0, 0x19);
        let mut v = || r.random_range(0, 5) + (70 - r.random_range(0, 5)) + shared;
        assert_eq!(m.strength[0], [v() as u8, v() as u8, v() as u8]);
        // 2. Bundesliga: scaled by 1.25 and clamped to 38..74.
        assert!(m.strength[20].iter().all(|&b| (38..=74).contains(&b)));
    }
}
