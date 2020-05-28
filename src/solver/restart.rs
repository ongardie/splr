//! Crate `restart` provides restart heuristics.
use {
    crate::{
        solver::{SearchStrategy, SolverEvent},
        types::*,
    },
    std::fmt,
};

/// API for restart condition.
trait ProgressEvaluator {
    /// map the value into a bool for forcing/blocking restart.
    fn is_active(&self) -> bool;
    /// reset internal state to the initial one.
    fn reset_progress(&mut self) {}
    /// calculate and set up the next condition.
    fn shift(&mut self);
}

/// Submodule index to access them indirectly.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum RestarterModule {
    Counter = 0,
    ASG,
    LBD,
    Luby,
    Reset,
}

/// Restart modes
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum RestartMode {
    /// Controlled by Glucose-like forcing and blocking restart scheme
    Dynamic = 0,
    /// Controlled by a good old scheme
    Luby,
    /// Controlled by CaDiCal-like Geometric Stabilizer
    Stabilize,
    // Bucket,
}

/// API for restart like `block_restart`, `force_restart` and so on.
pub trait RestartIF: Export<(RestartMode, usize, f64, f64, f64)> {
    /// return `true` if stabilizer is active.
    fn stabilizing(&self) -> bool;
    /// block restart if needed.
    fn block_restart(&mut self) -> bool;
    /// force restart if needed.
    fn force_restart(&mut self) -> bool;
    /// update specific submodule
    fn update(&mut self, kind: RestarterModule, val: usize);
}

/// An assignment history used for blocking restart.
#[derive(Debug)]
struct ProgressASG {
    enable: bool,
    asg: usize,
    ema: Ema,
    /// For block restart based on average assignments: 1.40.
    /// This is called `R` in Glucose
    threshold: f64,
}

impl Default for ProgressASG {
    fn default() -> ProgressASG {
        ProgressASG {
            enable: true,
            ema: Ema::new(1),
            asg: 0,
            threshold: 1.4,
        }
    }
}

impl Instantiate for ProgressASG {
    fn instantiate(config: &Config, _: &CNFDescription) -> Self {
        ProgressASG {
            ema: Ema::new(config.rst_asg_len),
            threshold: config.rst_asg_thr,
            ..ProgressASG::default()
        }
    }
}

impl EmaIF for ProgressASG {
    type Input = usize;
    fn update(&mut self, n: usize) {
        self.asg = n;
        self.ema.update(n as f64);
    }
    fn get(&self) -> f64 {
        self.ema.get()
    }
    fn trend(&self) -> f64 {
        (self.asg as f64) / self.ema.get()
    }
}

impl ProgressEvaluator for ProgressASG {
    fn is_active(&self) -> bool {
        self.enable && self.threshold * self.ema.get() < (self.asg as f64)
    }
    fn shift(&mut self) {}
}

/// An EMA of learnt clauses' LBD, used for forcing restart.
#[derive(Debug)]
struct ProgressLBD {
    enable: bool,
    ema: Ema2,
    num: usize,
    sum: usize,
    /// For force restart based on average LBD of newly generated clauses: 0.80.
    /// This is called `K` in Glucose
    threshold: f64,
}

impl Default for ProgressLBD {
    fn default() -> ProgressLBD {
        ProgressLBD {
            enable: true,
            ema: Ema2::new(1),
            num: 0,
            sum: 0,
            threshold: 1.4,
        }
    }
}

impl Instantiate for ProgressLBD {
    fn instantiate(config: &Config, _: &CNFDescription) -> Self {
        ProgressLBD {
            ema: Ema2::new(config.rst_lbd_len).with_slow(config.rst_lbd_slw),
            threshold: config.rst_lbd_thr,
            ..ProgressLBD::default()
        }
    }
}

impl EmaIF for ProgressLBD {
    type Input = usize;
    fn update(&mut self, d: usize) {
        self.num += 1;
        self.sum += d;
        self.ema.update(d as f64);
    }
    fn get(&self) -> f64 {
        self.ema.get()
    }
    fn trend(&self) -> f64 {
        self.ema
            .trend()
            .max(self.ema.get() * (self.num as f64) / (self.sum as f64))
    }
}

impl ProgressEvaluator for ProgressLBD {
    fn is_active(&self) -> bool {
        self.enable && self.threshold < self.ema.trend()
    }
    fn shift(&mut self) {}
}

/// An EMA of decision level.
#[derive(Debug)]
struct ProgressLVL {
    ema: Ema2,
}

impl Instantiate for ProgressLVL {
    fn instantiate(_: &Config, _: &CNFDescription) -> Self {
        ProgressLVL {
            ema: Ema2::new(100).with_slow(800),
        }
    }
}

impl EmaIF for ProgressLVL {
    type Input = usize;
    fn update(&mut self, l: usize) {
        self.ema.update(l as f64);
    }
    fn get(&self) -> f64 {
        self.ema.get()
    }
    fn trend(&self) -> f64 {
        self.ema.trend()
    }
}

impl ProgressEvaluator for ProgressLVL {
    fn is_active(&self) -> bool {
        todo!()
    }
    fn shift(&mut self) {}
}

/// An EMA of recurring conflict complexity (unused now).
#[derive(Debug)]
struct ProgressRCC {
    heat: Ema2,
    threshold: f64,
}

impl Default for ProgressRCC {
    fn default() -> Self {
        ProgressRCC {
            heat: Ema2::new(100).with_slow(8000),
            threshold: 0.0,
        }
    }
}

impl Instantiate for ProgressRCC {
    fn instantiate(_: &Config, _: &CNFDescription) -> Self {
        ProgressRCC::default()
    }
}

impl fmt::Display for ProgressRCC {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "ProgressRCC[heat:{}, thr:{}]",
            self.get(),
            self.threshold
        )
    }
}

impl EmaIF for ProgressRCC {
    type Input = f64;
    fn update(&mut self, foc: Self::Input) {
        self.heat.update(foc);
    }
    fn get(&self) -> f64 {
        self.heat.get()
    }
    fn trend(&self) -> f64 {
        self.heat.trend()
    }
}

impl ProgressEvaluator for ProgressRCC {
    fn is_active(&self) -> bool {
        self.threshold < self.heat.get()
    }
    fn shift(&mut self) {}
}

/// An implementation of Luby series.
#[derive(Debug)]
struct LubySeries {
    enable: bool,
    active: bool,
    index: usize,
    next_restart: usize,
    restart_inc: f64,
    step: usize,
}

impl Default for LubySeries {
    fn default() -> Self {
        const STEP: usize = 100;
        LubySeries {
            enable: false,
            active: false,
            index: 0,
            next_restart: STEP,
            restart_inc: 2.0,
            step: STEP,
        }
    }
}

impl Instantiate for LubySeries {
    fn instantiate(config: &Config, _: &CNFDescription) -> Self {
        LubySeries {
            step: config.rst_step,
            ..LubySeries::default()
        }
    }
}

impl fmt::Display for LubySeries {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.enable {
            write!(f, "Luby[index:{}, step:{}]", self.index, self.next_restart,)
        } else {
            write!(f, "Luby(deactive)")
        }
    }
}

impl EmaIF for LubySeries {
    type Input = usize;
    fn update(&mut self, index: usize) {
        if !self.enable {
            return;
        }
        if index == 0 {
            self.index = 0;
            self.next_restart = self.next_step();
            self.active = false;
        } else {
            self.active = self.next_restart < index;
        }
    }
    fn get(&self) -> f64 {
        self.next_restart as f64
    }
}

impl ProgressEvaluator for LubySeries {
    fn is_active(&self) -> bool {
        self.enable && self.active
    }
    fn shift(&mut self) {
        self.active = false;
        self.index += 1;
        self.next_restart = self.next_step();
    }
}

/// Find the finite subsequence that contains index 'x', and the
/// size of that subsequence as: 1, 1, 2, 1, 1, 2, 4, 1, 1, 2, 1, 1, 2, 4, 8
impl LubySeries {
    fn next_step(&self) -> usize {
        if self.index == 0 {
            return self.step;
        }
        let mut size: usize = 1;
        let mut seq: usize = 0;
        while size < self.index + 1 {
            seq += 1;
            size = 2 * size + 1;
        }
        let mut x = self.index;
        while size - 1 != x {
            size = (size - 1) >> 1;
            seq -= 1;
            x %= size;
        }
        (self.restart_inc.powf(seq as f64) * self.step as f64) as usize
    }
}

/// An implementation of Cadical-style blocker.
/// This is a stealth blocker between the other evaluators and solver;
/// the other evaluators work as if this blocker doesn't exist.
/// When an evaluator becomes active, we accept and shift it. But this blocker
/// absorbs not only the forcing signal but also blocking signal.
/// This exists in macro `reset`.
#[derive(Debug)]
struct GeometricStabilizer {
    enable: bool,
    active: bool,
    next_trigger: usize,
    restart_inc: f64,
    step: usize,
}

impl Default for GeometricStabilizer {
    fn default() -> Self {
        GeometricStabilizer {
            enable: true,
            active: false,
            next_trigger: 1000,
            restart_inc: 2.0,
            step: 1000,
        }
    }
}

impl Instantiate for GeometricStabilizer {
    fn instantiate(config: &Config, _: &CNFDescription) -> Self {
        GeometricStabilizer {
            enable: config.use_stabilize(),
            restart_inc: config.rst_stb_scl,
            ..GeometricStabilizer::default()
        }
    }
}

impl fmt::Display for GeometricStabilizer {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if !self.enable {
            write!(f, "Stabilizer(dead)")
        } else if self.active && self.enable {
            write!(f, "Stabilizer[+{}]", self.next_trigger)
        } else {
            write!(f, "Stabilizer[-{}]", self.next_trigger)
        }
    }
}

impl EmaIF for GeometricStabilizer {
    type Input = usize;
    fn update(&mut self, now: usize) {
        if self.enable && self.next_trigger <= now {
            self.active = !self.active;
            self.step = ((self.step as f64) * self.restart_inc) as usize;
            if 100_000_000 < self.step {
                self.step = 1000;
            }
            self.next_trigger += self.step;
        }
    }
    fn get(&self) -> f64 {
        todo!()
    }
}

impl ProgressEvaluator for GeometricStabilizer {
    fn is_active(&self) -> bool {
        self.enable && self.active
    }
    fn reset_progress(&mut self) {
        if self.enable {
            self.active = false;
            self.step = 1000;
        }
    }
    fn shift(&mut self) {}
}

/// Restart when LBD's sum is over a limit.
#[derive(Debug)]
struct ProgressBucket {
    enable: bool,
    num_shift: usize,
    sum: f64,
    power: f64,
    power_factor: f64,
    power_scale: f64,
    step: f64,
    threshold: f64,
}

impl Default for ProgressBucket {
    fn default() -> ProgressBucket {
        ProgressBucket {
            enable: false,
            num_shift: 0,
            sum: 0.0,
            power: 1.25,
            power_factor: 1.25,
            power_scale: 0.0,
            step: 1.0,
            threshold: 2000.0,
        }
    }
}

impl Instantiate for ProgressBucket {
    fn instantiate(config: &Config, _: &CNFDescription) -> Self {
        ProgressBucket {
            power: config.rst_bkt_pwr,
            power_factor: (config.rst_bkt_pwr - 1.0).max(0.0),
            power_scale: config.rst_bkt_scl,
            step: config.rst_bkt_inc,
            threshold: config.rst_bkt_thr as f64,
            ..ProgressBucket::default()
        }
    }
}

impl EmaIF for ProgressBucket {
    type Input = usize;
    fn update(&mut self, d: usize) {
        self.sum += (d as f64).powf(self.power);
    }
    fn get(&self) -> f64 {
        todo!()
    }
    fn trend(&self) -> f64 {
        todo!()
    }
}

impl ProgressEvaluator for ProgressBucket {
    fn is_active(&self) -> bool {
        self.enable && self.threshold < self.sum
    }
    fn reset_progress(&mut self) {
        if self.enable {
            self.num_shift = 1;
            self.threshold = 1000.0; // FIXME: we need the values in config
            self.shift();
        }
    }
    fn shift(&mut self) {
        self.num_shift += 1;
        self.sum = 0.0;
        self.threshold += self.step;
        // self.power = 1.0 + (self.num_shift as f64).powf(-0.2);
        // self.power = 1.0 + (1.0 + 0.001 * self.num_shift as f64).powf(-1.0);
        if 0.0 < self.power_factor {
            // If power_scale == 0.0, then p == 1.0 and power == config.rst_bkt_pwr.
            let p = (1.0 + self.power_scale * self.num_shift as f64).powf(-1.0);
            self.power = 1.0 + self.power_factor * p;
        }
    }
}

/// `Restarter` provides restart API and holds data about restart conditions.
#[derive(Debug)]
pub struct Restarter {
    asg: ProgressASG,
    // bkt: ProgressBucket,
    lbd: ProgressLBD,
    // pub rcc: ProgressRCC,
    // pub blvl: ProgressLVL,
    // pub clvl: ProgressLVL,
    luby: LubySeries,
    stb: GeometricStabilizer,
    after_restart: usize,
    next_restart: usize,
    restart_step: usize,

    //
    //## statistics
    //
    num_block: usize,
    num_stabilize: usize,
}

impl Default for Restarter {
    fn default() -> Restarter {
        Restarter {
            asg: ProgressASG::default(),
            // bkt: ProgressBucket::default(),
            lbd: ProgressLBD::default(),
            // rcc: ProgressRCC::default(),
            // blvl: ProgressLVL::default(),
            // clvl: ProgressLVL::default(),
            luby: LubySeries::default(),
            stb: GeometricStabilizer::default(),
            after_restart: 0,
            next_restart: 100,
            restart_step: 0,
            num_block: 0,
            num_stabilize: 0,
        }
    }
}

impl Instantiate for Restarter {
    fn instantiate(config: &Config, cnf: &CNFDescription) -> Self {
        Restarter {
            asg: ProgressASG::instantiate(config, cnf),
            // bkt: ProgressBucket::instantiate(config, cnf),
            lbd: ProgressLBD::instantiate(config, cnf),
            // rcc: ProgressRCC::instantiate(config, cnf),
            // blvl: ProgressLVL::instantiate(config, cnf),
            // clvl: ProgressLVL::instantiate(config, cnf),
            luby: LubySeries::instantiate(config, cnf),
            stb: GeometricStabilizer::instantiate(config, cnf),
            restart_step: config.rst_step,
            ..Restarter::default()
        }
    }
    fn handle(&mut self, e: SolverEvent) {
        if let SolverEvent::Adapt(strategy, num_conflict) = e {
            match strategy {
                (SearchStrategy::Initial, 0) => {
                    // self.int.enable = true;
                }
                (SearchStrategy::LowSuccesive, n) if n == num_conflict => self.luby.enable = true,
                _ => (),
            }
        }
    }
}

macro_rules! reset {
    ($executor: expr) => {
        $executor.after_restart = 0;
        // if $executor.stb.is_active() {
        //     $executor.num_block += 1;
        //     $executor.num_stabilize += 1;
        //     return false;
        // }
        return true;
    };
}

impl RestartIF for Restarter {
    fn stabilizing(&self) -> bool {
        self.stb.is_active()
    }
    fn block_restart(&mut self) -> bool {
        // || self.bkt.enable
        if self.after_restart < self.restart_step || self.luby.enable {
            return false;
        }
        if self.asg.is_active() {
            self.num_block += 1;
            reset!(self);
        }
        false
    }
    fn force_restart(&mut self) -> bool {
        if self.luby.is_active() {
            self.luby.shift();
            reset!(self);
        }
        /*
        if self.bkt.is_active() {
            self.bkt.shift();
            reset!(self);
        }
        */
        if self.after_restart < self.restart_step {
            return false;
        }
        if self.lbd.is_active() {
            self.lbd.shift();
            reset!(self);
        }
        false
    }
    fn update(&mut self, kind: RestarterModule, val: usize) {
        match kind {
            RestarterModule::Counter => {
                self.after_restart += 1;
                self.stb.update(val);
                self.luby.update(self.after_restart);
            }
            RestarterModule::ASG => self.asg.update(val),
            RestarterModule::LBD => {
                // self.bkt.update(val);
                self.lbd.update(val);
            }
            RestarterModule::Luby => self.luby.update(0),
            RestarterModule::Reset => (),
        }
    }
}

impl Export<(RestartMode, usize, f64, f64, f64)> for Restarter {
    /// exports:
    ///  1. current restart mode
    ///  1. the number of blocking restarts
    ///  1. `asg.trend()`
    ///  1. `lbd.get()`
    ///  1. `lbd.trend()`
    ///
    ///```
    /// use crate::splr::{config::Config, solver::Restarter, types::*};
    /// let rst = Restarter::instantiate(&Config::default(), &CNFDescription::default());
    /// let (_mode, _num_block, _asg_trend, _lbd_get, _lbd_trend) = rst.exports();
    ///```
    #[inline]
    fn exports(&self) -> (RestartMode, usize, f64, f64, f64) {
        (
            if self.stb.is_active() {
                RestartMode::Stabilize
            // } else if self.bkt.enable {
            //     RestartMode::Bucket
            } else if self.luby.enable {
                RestartMode::Luby
            } else {
                RestartMode::Dynamic
            },
            self.num_block,
            self.asg.trend(),
            self.lbd.get(),
            self.lbd.trend(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_luby_series() {
        let mut luby = LubySeries {
            enable: true,
            active: true,
            step: 1,
            ..LubySeries::default()
        };
        luby.update(0);
        for v in vec![1, 1, 2, 1, 1, 2, 4, 1, 1, 2, 1, 1, 2, 4, 8] {
            assert_eq!(luby.next_restart, v);
            luby.shift();
        }
    }
}