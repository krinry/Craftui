use craftui_core::Color;

/// Animation easing curves, tweens, and spring physics.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Easing {
    Linear,
    EaseIn,
    EaseOut,
    #[default]
    EaseInOut,
}

impl Easing {
    pub fn sample(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::Linear => t,
            Self::EaseIn => t * t * t,
            Self::EaseOut => {
                let inv = 1.0 - t;
                1.0 - inv * inv * inv
            }
            Self::EaseInOut => {
                if t < 0.5 {
                    4.0 * t * t * t
                } else {
                    1.0 - (-2.0 * t + 2.0).powi(3) * 0.5
                }
            }
        }
    }
}

/// Property identifier for per-node declarative and imperative transition specs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AnimatableProp {
    Opacity,
    TranslateX,
    TranslateY,
    ScaleX,
    ScaleY,
    Rotation,
    BackgroundColor,
    BorderColor,
}

/// Transition timing model: either a duration + easing curve or damped spring physics.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TransitionCurve {
    Tween { duration_sec: f32, easing: Easing },
    Spring { stiffness: f32, damping: f32 },
}

impl Default for TransitionCurve {
    fn default() -> Self {
        Self::Spring {
            stiffness: 280.0,
            damping: 26.0,
        }
    }
}

impl TransitionCurve {
    pub fn tween(duration_sec: f32, easing: Easing) -> Self {
        Self::Tween {
            duration_sec: duration_sec.max(0.001),
            easing,
        }
    }

    pub fn spring(stiffness: f32, damping: f32) -> Self {
        Self::Spring {
            stiffness: stiffness.max(1.0),
            damping: damping.max(0.1),
        }
    }

    pub fn snappy() -> Self {
        Self::Spring {
            stiffness: 420.0,
            damping: 30.0,
        }
    }

    pub fn gentle() -> Self {
        Self::Spring {
            stiffness: 200.0,
            damping: 24.0,
        }
    }

    pub fn instant() -> Self {
        Self::Tween {
            duration_sec: 0.001,
            easing: Easing::Linear,
        }
    }
}

/// Animated scalar value supporting retargetable tweens and velocity-preserving springs.
#[derive(Debug, Clone, Copy)]
pub struct AnimatedFloat {
    pub value: f32,
    pub target: f32,
    pub velocity: f32,
    pub curve: TransitionCurve,
    start_value: f32,
    elapsed_sec: f32,
    animating: bool,
}

impl AnimatedFloat {
    pub fn new(initial: f32, curve: TransitionCurve) -> Self {
        Self {
            value: initial,
            target: initial,
            velocity: 0.0,
            curve,
            start_value: initial,
            elapsed_sec: 0.0,
            animating: false,
        }
    }

    pub fn set_curve(&mut self, curve: TransitionCurve) {
        self.curve = curve;
    }

    pub fn set_target(&mut self, target: f32) {
        if (target - self.target).abs() <= 1e-4
            && (!self.animating || (target - self.value).abs() <= 1e-4)
        {
            return;
        }
        self.target = target;
        match self.curve {
            TransitionCurve::Tween { .. } => {
                self.start_value = self.value;
                self.elapsed_sec = 0.0;
                self.velocity = 0.0;
                self.animating = (self.target - self.value).abs() > 1e-4;
                if !self.animating {
                    self.value = self.target;
                }
            }
            TransitionCurve::Spring { .. } => {
                self.animating =
                    (self.target - self.value).abs() > 0.0005 || self.velocity.abs() > 0.005;
                if !self.animating {
                    self.value = self.target;
                    self.velocity = 0.0;
                }
            }
        }
    }

    pub fn snap(&mut self, value: f32) {
        self.value = value;
        self.target = value;
        self.start_value = value;
        self.velocity = 0.0;
        self.elapsed_sec = 0.0;
        self.animating = false;
    }

    pub fn is_animating(&self) -> bool {
        self.animating
    }

    /// Advance animation by `dt_sec`. Returns `true` if `value` changed during this step
    /// (including the final settle frame; check [`Self::is_animating`] for whether more frames are needed).
    pub fn step(&mut self, dt_sec: f32) -> bool {
        if !self.animating {
            return false;
        }
        let dt_total = dt_sec.clamp(0.0001, 0.05);
        match self.curve {
            TransitionCurve::Tween {
                duration_sec,
                easing,
            } => {
                let dur = duration_sec.max(0.001);
                self.elapsed_sec = (self.elapsed_sec + dt_total).min(dur);
                let t = easing.sample(self.elapsed_sec / dur);
                self.value = self.start_value + (self.target - self.start_value) * t;
                if self.elapsed_sec >= dur - 1e-6 {
                    self.value = self.target;
                    self.animating = false;
                }
                true
            }
            TransitionCurve::Spring { stiffness, damping } => {
                let steps = ((dt_total / 0.008).ceil() as u32).clamp(1, 8);
                let dt = dt_total / steps as f32;
                for _ in 0..steps {
                    let force = -stiffness * (self.value - self.target);
                    let damp = -damping * self.velocity;
                    let accel = force + damp;
                    self.velocity += accel * dt;
                    self.value += self.velocity * dt;
                }
                let settled =
                    (self.value - self.target).abs() < 0.0005 && self.velocity.abs() < 0.005;
                if settled {
                    self.value = self.target;
                    self.velocity = 0.0;
                    self.animating = false;
                }
                true
            }
        }
    }
}

/// Animated RGBA [`Color`] smoothly interpolating when its target color changes.
#[derive(Debug, Clone, Copy)]
pub struct AnimatedColor {
    pub value: Color,
    from: Color,
    target: Color,
    progress: AnimatedFloat,
}

impl AnimatedColor {
    pub fn new(initial: Color, curve: TransitionCurve) -> Self {
        Self {
            value: initial,
            from: initial,
            target: initial,
            progress: AnimatedFloat::new(1.0, curve),
        }
    }

    pub fn target(&self) -> Color {
        self.target
    }

    pub fn set_curve(&mut self, curve: TransitionCurve) {
        self.progress.set_curve(curve);
    }

    pub fn set_target(&mut self, target: Color) {
        if target == self.target {
            return;
        }
        self.from = self.value;
        self.target = target;
        self.progress.snap(0.0);
        self.progress.set_target(1.0);
    }

    pub fn snap(&mut self, color: Color) {
        self.value = color;
        self.from = color;
        self.target = color;
        self.progress.snap(1.0);
    }

    pub fn is_animating(&self) -> bool {
        self.progress.is_animating()
    }

    pub fn step(&mut self, dt_sec: f32) -> bool {
        if !self.progress.step(dt_sec) {
            return false;
        }
        let t = self.progress.value.clamp(0.0, 1.0);
        self.value = self.from.lerp(self.target, t);
        if !self.progress.is_animating() {
            self.value = self.target;
        }
        true
    }
}

/// Time-based tween animation.
#[derive(Debug, Clone, Copy)]
pub struct Tween {
    pub from: f32,
    pub to: f32,
    pub duration_sec: f32,
    pub elapsed_sec: f32,
    pub easing: Easing,
}

impl Tween {
    pub fn new(from: f32, to: f32, duration_sec: f32, easing: Easing) -> Self {
        Self {
            from,
            to,
            duration_sec: duration_sec.max(0.001),
            elapsed_sec: 0.0,
            easing,
        }
    }

    pub fn step(&mut self, dt_sec: f32) -> (f32, bool) {
        self.elapsed_sec = (self.elapsed_sec + dt_sec).min(self.duration_sec);
        let t = self.easing.sample(self.elapsed_sec / self.duration_sec);
        let value = self.from + (self.to - self.from) * t;
        let active = self.elapsed_sec < self.duration_sec;
        (value, active)
    }
}

/// Physical configuration parameters for damped harmonic spring simulations.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpringConfig {
    pub mass: f32,
    pub stiffness: f32,
    pub damping: f32,
}

impl Default for SpringConfig {
    fn default() -> Self {
        Self::smooth()
    }
}

impl SpringConfig {
    pub fn new(mass: f32, stiffness: f32, damping: f32) -> Self {
        Self {
            mass: mass.max(0.001),
            stiffness: stiffness.max(0.1),
            damping: damping.max(0.0),
        }
    }

    /// Critically-damped smooth spring without overshoot.
    pub fn smooth() -> Self {
        let stiffness: f32 = 260.0;
        let mass: f32 = 1.0;
        let damping = 2.0 * (mass * stiffness).sqrt(); // zeta = 1.0
        Self {
            mass,
            stiffness,
            damping,
        }
    }

    /// Snappy spring for quick responsive button taps and micro-interactions.
    pub fn snappy() -> Self {
        Self {
            mass: 1.0,
            stiffness: 420.0,
            damping: 32.0,
        }
    }

    /// Bouncy spring with pleasant under-damped oscillation.
    pub fn bouncy() -> Self {
        Self {
            mass: 1.0,
            stiffness: 280.0,
            damping: 16.0,
        }
    }

    /// Gentle, floating spring for large page transitions or modal slides.
    pub fn gentle() -> Self {
        Self {
            mass: 1.0,
            stiffness: 160.0,
            damping: 22.0,
        }
    }

    /// Highly responsive spring with minimal latency for touch drag gestures.
    pub fn interactive() -> Self {
        Self {
            mass: 1.0,
            stiffness: 580.0,
            damping: 42.0,
        }
    }

    /// Damping ratio ($\zeta$).
    pub fn damping_ratio(&self) -> f32 {
        self.damping / (2.0 * (self.mass * self.stiffness).sqrt())
    }

    /// Undamped angular frequency ($\omega_0 = \sqrt{k/m}$).
    pub fn natural_frequency(&self) -> f32 {
        (self.stiffness / self.mass).sqrt()
    }
}

/// Exact analytical solution to the 2nd-order damped harmonic oscillator differential equation.
///
/// Evaluates position and velocity in $O(1)$ time with zero cumulative numerical drift or instability.
#[derive(Debug, Clone, Copy)]
pub struct SpringSimulation {
    config: SpringConfig,
    start_value: f32,
    target_value: f32,
    initial_velocity: f32,
}

impl SpringSimulation {
    pub fn new(config: SpringConfig, start: f32, target: f32, initial_velocity: f32) -> Self {
        Self {
            config,
            start_value: start,
            target_value: target,
            initial_velocity,
        }
    }

    /// Sample position and velocity at continuous time `t` (seconds).
    pub fn sample(&self, t: f32) -> (f32, f32) {
        if t <= 0.0 {
            return (self.start_value, self.initial_velocity);
        }

        let zeta = self.config.damping_ratio();
        let w0 = self.config.natural_frequency();
        let x0 = self.start_value - self.target_value;
        let v0 = self.initial_velocity;

        if (zeta - 1.0).abs() < 1e-4 {
            // Critically damped (zeta == 1.0)
            let c1 = x0;
            let c2 = v0 + w0 * x0;
            let decay = (-w0 * t).exp();
            let pos = self.target_value + decay * (c1 + c2 * t);
            let vel = decay * (c2 - w0 * (c1 + c2 * t));
            (pos, vel)
        } else if zeta < 1.0 {
            // Under-damped (zeta < 1.0): oscillates with decaying exponential envelope
            let wd = w0 * (1.0 - zeta * zeta).sqrt();
            let c1 = x0;
            let c2 = (v0 + zeta * w0 * x0) / wd;
            let decay = (-zeta * w0 * t).exp();
            let cos_wt = (wd * t).cos();
            let sin_wt = (wd * t).sin();
            let pos = self.target_value + decay * (c1 * cos_wt + c2 * sin_wt);
            let vel =
                decay * ((wd * c2 - zeta * w0 * c1) * cos_wt - (wd * c1 + zeta * w0 * c2) * sin_wt);
            (pos, vel)
        } else {
            // Over-damped (zeta > 1.0): two distinct decaying real exponential roots.
            // Using r1 = -w0 / (zeta + sqrt(zeta^2 - 1)) avoids catastrophic cancellation
            // when zeta >> 1.0 (where f32 sqrt(zeta^2 - 1) rounds to zeta, yielding r1 = 0).
            let sqrt_term = (zeta * zeta - 1.0).sqrt();
            let sum_term = zeta + sqrt_term;
            let r1 = -w0 / sum_term;
            let r2 = -w0 * sum_term;
            let c1 = (v0 - r2 * x0) / (r1 - r2);
            let c2 = x0 - c1;
            let e1 = (r1 * t).exp();
            let e2 = (r2 * t).exp();
            let pos = self.target_value + c1 * e1 + c2 * e2;
            let vel = c1 * r1 * e1 + c2 * r2 * e2;
            (pos, vel)
        }
    }

    /// Check if the simulation has settled at time `t` within position tolerance (0.001) and velocity tolerance (0.005).
    pub fn is_settled(&self, t: f32) -> bool {
        let (pos, vel) = self.sample(t);
        (pos - self.target_value).abs() < 0.001 && vel.abs() < 0.005
    }
}

/// Damped harmonic spring physics solver for smooth UI transitions and overscroll bounce.
#[derive(Debug, Clone, Copy)]
pub struct Spring {
    pub value: f32,
    pub target: f32,
    pub velocity: f32,
    pub stiffness: f32,
    pub damping: f32,
}

impl Default for Spring {
    fn default() -> Self {
        Self::new(0.0, 240.0, 26.0)
    }
}

impl Spring {
    pub fn new(initial: f32, stiffness: f32, damping: f32) -> Self {
        Self {
            value: initial,
            target: initial,
            velocity: 0.0,
            stiffness,
            damping,
        }
    }

    pub fn set_target(&mut self, target: f32) {
        self.target = target;
    }

    /// Advance spring by `dt_sec`. Returns `true` if still animating.
    pub fn step(&mut self, dt_sec: f32) -> bool {
        let dt_total = dt_sec.clamp(0.0001, 0.05);
        let steps = ((dt_total / 0.008).ceil() as u32).clamp(1, 8);
        let dt = dt_total / steps as f32;
        for _ in 0..steps {
            let force = -self.stiffness * (self.value - self.target);
            let damp = -self.damping * self.velocity;
            let accel = force + damp;
            self.velocity += accel * dt;
            self.value += self.velocity * dt;
        }

        let settled = (self.value - self.target).abs() < 0.0005 && self.velocity.abs() < 0.005;
        if settled {
            self.value = self.target;
            self.velocity = 0.0;
            false
        } else {
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spring_converges_to_target() {
        let mut s = Spring::new(0.0, 220.0, 24.0);
        s.set_target(100.0);
        for _ in 0..240 {
            if !s.step(1.0 / 60.0) {
                break;
            }
        }
        assert!((s.value - 100.0).abs() < 0.01);
    }

    #[test]
    fn analytical_spring_simulation_under_and_critically_damped() {
        // Critically damped simulation
        let sim_crit = SpringSimulation::new(SpringConfig::smooth(), 0.0, 100.0, 0.0);
        assert_eq!(sim_crit.sample(0.0).0, 0.0);
        let (mid_pos, _) = sim_crit.sample(0.2);
        assert!(mid_pos > 50.0 && mid_pos < 100.0);
        assert!(sim_crit.is_settled(1.5));
        assert!((sim_crit.sample(2.0).0 - 100.0).abs() < 0.001);

        // Bouncy (under-damped) simulation should overshoot 100
        let sim_bouncy = SpringSimulation::new(SpringConfig::bouncy(), 0.0, 100.0, 0.0);
        assert!(sim_bouncy.config.damping_ratio() < 1.0);
        let mut overshot = false;
        for i in 1..100 {
            let t = i as f32 * 0.01;
            let (pos, _) = sim_bouncy.sample(t);
            if pos > 100.0 {
                overshot = true;
                break;
            }
        }
        assert!(
            overshot,
            "Under-damped bouncy spring should overshoot target"
        );
        assert!(sim_bouncy.is_settled(2.0));
    }

    #[test]
    fn overdamped_spring_stable_cancellation_test() {
        // High damping ratio zeta = 32768 / (2 * sqrt(1 * 1)) = 16384.0
        let cfg = SpringConfig::new(1.0, 1.0, 32768.0);
        let sim = SpringSimulation::new(cfg, 0.0, 1.0, 0.0);
        let (pos, _) = sim.sample(32768.0);
        // At t = 32768, pos should be approx 1 - e^(-1) ≈ 0.632
        assert!(
            (pos - 0.632).abs() < 0.05,
            "Overdamped spring should reach ~0.632 at t=32768, got {}",
            pos
        );
    }

    #[test]
    fn animated_float_tween_and_color_retarget_smoothly() {
        let mut f = AnimatedFloat::new(0.0, TransitionCurve::tween(0.16, Easing::EaseOut));
        f.set_target(1.0);
        assert!(f.is_animating());
        assert!(f.step(0.05));
        assert!(f.value > 0.2 && f.value < 0.99);
        f.set_target(0.25);
        for _ in 0..20 {
            f.step(0.016);
        }
        assert!(!f.is_animating());
        assert!((f.value - 0.25).abs() < 1e-4);

        let mut c = AnimatedColor::new(Color::BLACK, TransitionCurve::tween(0.1, Easing::Linear));
        c.set_target(Color::WHITE);
        assert!(c.step(0.05));
        assert!(c.value.r > 100 && c.value.r < 160);
        for _ in 0..10 {
            c.step(0.02);
        }
        assert_eq!(c.value, Color::WHITE);
    }
}
