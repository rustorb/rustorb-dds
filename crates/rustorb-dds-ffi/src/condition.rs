//! Conditions (`rdds_condition`: guard, status and read conditions) and
//! WaitSets (`rdds_waitset`).

use rustorb_dds::{Condition, GuardCondition, StatusMask, WaitSet};

use crate::entity::RddsDuration;
use crate::{Res, Status, bad, guard, guard_value, obj, put, release, set};

/// `rdds_condition`.
#[derive(Debug, Clone)]
pub struct RddsCondition(pub(crate) Condition);

impl<T: Into<Condition>> From<T> for RddsCondition {
    fn from(c: T) -> Self {
        RddsCondition(c.into())
    }
}

/// `rdds_guard_condition_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_guard_condition_new(out: *mut *mut RddsCondition) -> Status {
    guard(|| unsafe { put(out, RddsCondition::from(GuardCondition::new())) })
}

/// `rdds_condition_release`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_condition_release(c: *mut RddsCondition) {
    unsafe { release(c) }
}

/// `rdds_condition_trigger_value`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_condition_trigger_value(c: *const RddsCondition) -> bool {
    guard_value(false, || {
        unsafe { c.as_ref() }.is_some_and(|c| c.0.trigger_value())
    })
}

/// `rdds_condition_equals`: whether two handles are the same condition.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_condition_equals(
    a: *const RddsCondition,
    b: *const RddsCondition,
) -> bool {
    guard_value(false, || match unsafe { (a.as_ref(), b.as_ref()) } {
        (Some(a), Some(b)) => a.0 == b.0,
        _ => false,
    })
}

fn guard_condition(c: &RddsCondition) -> Res<&GuardCondition> {
    match &c.0 {
        Condition::Guard(g) => Ok(g),
        _ => Err(bad("not a guard condition")),
    }
}

/// `rdds_guard_condition_set_trigger_value`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_guard_condition_set_trigger_value(
    c: *const RddsCondition,
    value: bool,
) -> Status {
    guard(|| {
        let c = unsafe { obj(c, "the condition") }?;
        guard_condition(c)?.set_trigger_value(value);
        Ok(())
    })
}

/// `rdds_status_condition_get_enabled_statuses`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_status_condition_get_enabled_statuses(
    c: *const RddsCondition,
    out: *mut u32,
) -> Status {
    guard(|| {
        let c = unsafe { obj(c, "the condition") }?;
        let Condition::Status(s) = &c.0 else {
            return Err(bad("not a status condition"));
        };
        unsafe { set(out, s.enabled_statuses().0) }
    })
}

/// `rdds_status_condition_set_enabled_statuses`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_status_condition_set_enabled_statuses(
    c: *const RddsCondition,
    mask: u32,
) -> Status {
    guard(|| {
        let c = unsafe { obj(c, "the condition") }?;
        let Condition::Status(s) = &c.0 else {
            return Err(bad("not a status condition"));
        };
        s.set_enabled_statuses(StatusMask(mask));
        Ok(())
    })
}

/// `rdds_waitset_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_waitset_new(out: *mut *mut WaitSet) -> Status {
    guard(|| unsafe { put(out, WaitSet::new()) })
}

/// `rdds_waitset_release`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_waitset_release(w: *mut WaitSet) {
    unsafe { release(w) }
}

/// `rdds_waitset_attach_condition`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_waitset_attach_condition(
    w: *const WaitSet,
    c: *const RddsCondition,
) -> Status {
    guard(|| {
        let w = unsafe { obj(w, "the waitset") }?;
        let c = unsafe { obj(c, "the condition") }?;
        Ok(w.attach_condition(c.0.clone())?)
    })
}

/// `rdds_waitset_detach_condition`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_waitset_detach_condition(
    w: *const WaitSet,
    c: *const RddsCondition,
) -> Status {
    guard(|| {
        let w = unsafe { obj(w, "the waitset") }?;
        let c = unsafe { obj(c, "the condition") }?;
        Ok(w.detach_condition(c.0.clone())?)
    })
}

/// `rdds_waitset_wait`: until a condition is true, or `RETCODE_TIMEOUT`.
/// The indices (in the order the conditions were attached, counting
/// those still attached) of the true ones go into `active`, up to
/// `cap`, and how many there are into `*len`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_waitset_wait(
    w: *const WaitSet,
    timeout: RddsDuration,
    active: *mut usize,
    cap: usize,
    len: *mut usize,
) -> Status {
    guard(|| {
        let w = unsafe { obj(w, "the waitset") }?;
        let t = timeout_of(timeout)?;
        let on = w.wait(t)?;
        let all = w.conditions();
        let idx: Vec<usize> = all
            .iter()
            .enumerate()
            .filter(|(_, c)| on.contains(c))
            .map(|(i, _)| i)
            .collect();
        for (i, x) in idx.iter().take(cap).enumerate() {
            // SAFETY: room for `cap` (the header).
            unsafe { active.add(i).write(*x) };
        }
        unsafe { set(len, idx.len()) }
    })
}

fn timeout_of(d: RddsDuration) -> Res<std::time::Duration> {
    if d == crate::entity::DURATION_INFINITE {
        return Ok(std::time::Duration::from_secs(u32::MAX.into()));
    }
    if d.sec < 0 || d.nanosec >= 1_000_000_000 {
        return Err(bad("a duration out of range"));
    }
    Ok(std::time::Duration::new(d.sec as u64, d.nanosec))
}
