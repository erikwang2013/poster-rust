//! 行为轨迹校验（slider / rotate），对应 PHP `TrajectoryVerifier`。
//!
//! `captcha.trajectory.enabled` 默认关闭。开启后旧前端（只提交数值、不带轨迹）一律判失败，
//! 属于有意的破坏性变更；灰度前请确认触屏 / 手写笔 / 无障碍工具 / 远程桌面等真实用户
//! 的轨迹不会被误杀。
//!
//! 判定条件（需全部满足）：
//! 1. 采样点数 >= `min_points`；
//! 2. `duration` 落在 `[min_duration, max_duration]`（毫秒）；
//! 3. 线性度 <= `max_linearity`：轨迹在拖动方向上的位移投影 vs 时间做最小二乘拟合取 R²，
//!    脚本按直线匀速插值时 R² ≈ 1，人手的加减速与抖动会显著拉低 R²。

use crate::config::TrajectoryOptions;

/// 一次交互的轨迹：`(x, y, t)` 采样点 + 总耗时（毫秒）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Trajectory {
    /// 采样点，`t` 为该点相对起点的毫秒数。
    pub trail: Vec<(f64, f64, f64)>,
    /// 交互总耗时（毫秒）。
    pub duration_ms: f64,
}

impl Trajectory {
    /// 构造。
    pub fn new(trail: Vec<(f64, f64, f64)>, duration_ms: f64) -> Self {
        Self { trail, duration_ms }
    }
}

/// 判定本次交互是否「像人」。
///
/// `trail` 为 `None`（旧前端只提交数值）时：校验关闭 → `true`，校验开启 → `false`。
pub fn verify(opts: &TrajectoryOptions, trail: Option<&Trajectory>) -> bool {
    if !opts.enabled {
        return true;
    }
    let Some(t) = trail else {
        return false;
    };
    if t.trail.len() < opts.min_points
        || t.duration_ms < opts.min_duration_ms as f64
        || t.duration_ms > opts.max_duration_ms as f64
    {
        return false;
    }
    linearity(&t.trail) <= opts.max_linearity as f64
}

/// 线性度 = 位移投影 vs 时间的最小二乘拟合 R²。越大越像机器。
///
/// 退化情形（全程无位移 / 时间戳全相同 / 位移无方差）返回 1.0，按机器处理。
pub fn linearity(points: &[(f64, f64, f64)]) -> f64 {
    let Some(first) = points.first().copied() else {
        return 1.0;
    };
    let last = points[points.len() - 1];
    let (dx, dy) = (last.0 - first.0, last.1 - first.1);
    let length = (dx * dx + dy * dy).sqrt();
    if length <= 0.0 {
        return 1.0;
    }

    let count = points.len() as f64;
    let (mut sum_t, mut sum_s, mut sum_tt, mut sum_ts) = (0.0, 0.0, 0.0, 0.0);
    let mut orthogonal = Vec::with_capacity(points.len());
    for point in points {
        // 投影到弦方向：旋转轨迹的位移可能落在 y 上，垂直方向的抖动不计入时间线性度
        let s = ((point.0 - first.0) * dx + (point.1 - first.1) * dy) / length;
        let t = point.2;
        orthogonal.push((t, s));
        sum_t += t;
        sum_s += s;
        sum_tt += t * t;
        sum_ts += t * s;
    }

    let denominator = count * sum_tt - sum_t * sum_t;
    if denominator.abs() < 1e-9 {
        return 1.0;
    }
    let slope = (count * sum_ts - sum_t * sum_s) / denominator;
    let intercept = (sum_s - slope * sum_t) / count;
    let mean = sum_s / count;

    let (mut residual, mut total) = (0.0, 0.0);
    for (t, s) in orthogonal {
        let predicted = slope * t + intercept;
        residual += (s - predicted).powi(2);
        total += (s - mean).powi(2);
    }
    if total < 1e-9 {
        return 1.0;
    }

    1.0 - residual / total
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(enabled: bool) -> TrajectoryOptions {
        TrajectoryOptions {
            enabled,
            ..Default::default()
        }
    }

    #[test]
    fn disabled_accepts_anything_including_no_trail() {
        assert!(verify(&opts(false), None));
        assert!(verify(&opts(false), Some(&Trajectory::new(vec![], 0.0))));
    }

    #[test]
    fn enabled_rejects_missing_trail() {
        assert!(!verify(&opts(true), None));
    }

    #[test]
    fn enabled_rejects_too_few_points_and_bad_duration() {
        let o = opts(true);
        let short = Trajectory::new(vec![(0.0, 0.0, 0.0), (10.0, 0.0, 50.0)], 400.0);
        assert!(!verify(&o, Some(&short)), "点数不足");

        // 先停一下再冲过去：人手常见的加减速，线性度约 0.77
        let human = || {
            Trajectory::new(
                vec![
                    (0.0, 0.0, 0.0),
                    (5.0, 0.0, 150.0),
                    (6.0, 1.0, 300.0),
                    (30.0, 0.0, 450.0),
                ],
                500.0,
            )
        };
        assert!(verify(&o, Some(&human())), "带加减速的轨迹应通过");

        let mut fast = human();
        fast.duration_ms = 100.0;
        assert!(!verify(&o, Some(&fast)), "耗时过短");

        let mut slow = human();
        slow.duration_ms = 9000.0;
        assert!(!verify(&o, Some(&slow)), "耗时过长");
    }

    #[test]
    fn machine_like_linear_trail_is_rejected() {
        let o = opts(true);
        // 匀速直线插值：R² ≈ 1
        let trail: Vec<(f64, f64, f64)> = (0..=10)
            .map(|i| (i as f64 * 10.0, 0.0, i as f64 * 50.0))
            .collect();
        let linear = Trajectory::new(trail, 500.0);
        assert!(linearity(&linear.trail) > 0.999);
        assert!(!verify(&o, Some(&linear)));
    }

    #[test]
    fn degenerate_trails_count_as_machine() {
        assert_eq!(linearity(&[]), 1.0);
        assert_eq!(linearity(&[(1.0, 2.0, 3.0)]), 1.0, "只有一个点：无位移");
        assert_eq!(
            linearity(&[(0.0, 0.0, 0.0), (0.0, 0.0, 10.0), (0.0, 0.0, 20.0)]),
            1.0,
            "无位移"
        );
        // 时间戳全相同 → 分母为 0
        assert_eq!(
            linearity(&[(0.0, 0.0, 5.0), (10.0, 0.0, 5.0), (20.0, 0.0, 5.0)]),
            1.0
        );
    }

    #[test]
    fn wobbly_trail_has_low_linearity() {
        let trail = vec![
            (0.0, 0.0, 0.0),
            (5.0, 0.0, 120.0),
            (30.0, 0.0, 480.0),
            (60.0, 0.0, 500.0),
        ];
        let r2 = linearity(&trail);
        assert!(r2 < 0.9, "加减速应拉低线性度, got {r2}");
    }
}
