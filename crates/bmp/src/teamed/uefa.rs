//! Package "uefa": the strength list of the foreign clubs (spec:
//! `docs/re/teamed/uefa.md`, to be written by the package).

use crate::host::{Host, Run};

use super::Editor;

/// "UEFA" on the bar: the list of the 136 foreign clubs with their three strengths.
///
/// TODO(#158) package uefa: port 12a5:233d (with `1000:1bd2` and `12a5:2ce0`, the
/// "UEFA-CUP SONST ZU LEICHT." check). The skeleton returns at once.
///
/// Origin: TEAMED.EXE 12a5:233d (uefa_screen)
pub fn run(host: &mut Host, ed: &mut Editor) -> Run<()> {
    let _ = (host, ed);
    Ok(())
}
