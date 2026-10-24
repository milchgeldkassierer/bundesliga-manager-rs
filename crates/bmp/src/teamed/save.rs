//! "PROGRAMM-ENDE": the save question and writing `MANA.DAT`.

use bmp_formats::mana::{self, ManaDat};
use bmp_game::rng::{Rng, clamp_int, ldiv};

use crate::host::{Host, Run};

use super::Editor;
use super::dialog::{self, Choice};

/// Outcome of [`teamed_save_mana`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveResult {
    /// "MACH MAL !": the file was written; the program ends.
    Saved,
    /// "OH NEIN !": nothing written; the program ends.
    NotSaved,
    /// Click outside the question (9): back to the editor.
    Cancelled,
}

/// Asks "Daten / unwiederbringlich / abspeichern ?" and on "MACH MAL !" recomputes the
/// strengths of the German clubs and writes all blocks of `MANA.DAT`.
///
/// Origin: TEAMED.EXE 1579:0030 (teamed_save_mana)
pub fn teamed_save_mana(host: &mut Host, ed: &mut Editor) -> Run<SaveResult> {
    let choice = dialog::two_choice(
        host,
        ed,
        [b"Daten", b"unwiederbringlich", b"abspeichern ?"],
        b"MACH MAL !",
        b"OH NEIN !",
    )?;
    match choice {
        Choice::Right => return Ok(SaveResult::NotSaved),
        Choice::Outside => return Ok(SaveResult::Cancelled),
        Choice::Left => {}
    }
    recompute_strengths(&mut ed.mana, &mut ed.rng);
    let path = host.data.join("MANA.DAT");
    // The original ignores write errors (file_open/file_write results are not checked
    // after the open succeeded); the port reports them on stderr and carries on.
    if let Err(e) = std::fs::write(&path, ed.mana.to_bytes()) {
        eprintln!("{}: {e}", path.display());
    }
    Ok(SaveResult::Saved)
}

/// Base strength by group (`DS:0554`: Bundesliga, 2. Bundesliga, amateurs).
pub const GROUP_BASE: [i8; 3] = [70, 34, 20];
/// Clubs per league by group (`DS:033C`); the rank term is `size - rank`.
pub const GROUP_SIZE: [i8; 3] = [18, 20, 20];

/// Group of German club `i` for the recomputation: `(i > 37) + (i > 17)`.
pub fn strength_group(i: usize) -> usize {
    usize::from(i > 0x25) + usize::from(i > 0x11)
}

/// The strength bytes of clubs 0..63 from the points, goals and rank of the previous
/// season (and `random_range`), see `docs/re/teamed/uefa.md` "Strength recomputation".
///
/// Per group the averages of the plus points, goals scored and goals conceded over all
/// 64 clubs of the group are divided by the number of clubs with a rank (0 stays the
/// sum, an average of 0 becomes 1). Clubs without a rank and the amateurs (group 2) get
/// `base + rand(0..5) - rand(0..5) + rand(0..25)` three times (one `rand(0..25)` shared);
/// the others get values from the rank, the points and the goals. The 2. Bundesliga is
/// then scaled by 125/100 and clamped to 38..74; all values are clamped to 20..95. The
/// foreign clubs 64..199 keep their bytes.
///
/// Origin: TEAMED.EXE 1579:006d (teamed_save_mana, strength recomputation)
pub fn recompute_strengths(m: &mut ManaDat, rng: &mut Rng) {
    let mut ranked = [0i16; 3];
    let mut points = [0i16; 3];
    let mut scored = [0i16; 3];
    let mut conceded = [0i16; 3];
    for i in 0..mana::GERMAN_CLUB_COUNT {
        let g = strength_group(i);
        points[g] = points[g].wrapping_add(i16::from(m.points[i][0]));
        scored[g] = scored[g].wrapping_add(i16::from(m.goals[i][0]));
        conceded[g] = conceded[g].wrapping_add(i16::from(m.goals[i][1]));
        if m.rank[i] != 0 {
            ranked[g] += 1;
        }
    }
    for g in 0..3 {
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
        let g = strength_group(i);
        let base = i16::from(GROUP_BASE[g]);
        // The three bytes in file order ([BP-0x68], [BP-0x66], [BP-0x7A]).
        let (mut first, mut second, mut third);
        if m.rank[i] == 0 || g == 2 {
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
            let form = i16::from(GROUP_SIZE[g]) - i16::from(m.rank[i]) - table / 20;
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
        assert_eq!(strength_group(17), 0);
        assert_eq!(strength_group(18), 1);
        assert_eq!(strength_group(37), 1);
        assert_eq!(strength_group(38), 2);
        assert_eq!(strength_group(63), 2);
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
            if shipped.rank[i] != 0 && strength_group(i) != 2 {
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
