use std::process::Command;

use itertools::Itertools;

pub fn get_width_height(path: String) -> Option<(usize, usize, Option<i32>)> {
    let o = Command::new("ffprobe")
        .args(["-v", "error"])
        .args(["-select_streams", "v:0"])
        .args(["-show_entries", "stream=width,height,r_frame_rate"])
        .args(["-of", "csv=s=x:p=0"])
        .arg(path)
        .output()
        .unwrap();

    let s = std::str::from_utf8(&o.stdout).unwrap();

    match s.trim().split('x').collect_vec()[..] {
        [a, b, c] => Some((a.parse().unwrap(), b.parse().unwrap(), {
            let (x, y) = c.split('/').collect_tuple().unwrap();
            Some(x.parse::<i32>().unwrap() / y.parse::<i32>().unwrap())
        })),
        [a, b] => Some((a.trim().parse().unwrap(), b.trim().parse().unwrap(), None)),
        _ => None,
    }
}
