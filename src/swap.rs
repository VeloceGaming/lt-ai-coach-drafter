//! Swap-stage delegation: let LT Coach's arrangement replace the one the game's own
//! swap-stage Coach button computes.
//!
//! What the static analysis and the log-only probe established (LTTM2/lt_swap_re/FINDINGS.md,
//! Rounds 3-4; game 0.6.2, no longer supported), re-established for each build in [`PROFILES`]:
//!
//! * the button runs `MatchUIRunner::apply_coach_swap_order`, which calls
//!   `compute_coach_swap_order` (RVA `0x2505cf0` in 0.6.2) and stores the returned `Vec<usize>`
//!   as the team's row arrangement; the order is an **absolute target**, deterministic,
//!   `order[row]` = draft-order index of the champion that row should hold (identity
//!   `[0,1,2,3,4]` before any swap; `[2,1,0,3,4]` swaps Top and Mid);
//! * `compute` has exactly two callers, both plain `CALL rel32`: the apply path and
//!   the coach-dialogue text builder.
//!
//! This module redirects those two calls to a wrapper that **always calls the original
//! first**, with the caller's own arguments, and only then — when every check passes —
//! overwrites the five elements of the returned vector in place (no allocation, same
//! length, still a permutation). Anything unexpected leaves the game's own answer alone.
//!
//! Safety rails (as in LTDirectControl's adapter): the executable's SHA-256, PE
//! timestamp and image size must match one reviewed build in [`PROFILES`]; both call
//! sites must hold exactly the expected bytes and resolve to the expected target;
//! patching happens only on the title screen; the executable on disk is never touched;
//! the DLL is pinned so the relays stay valid; a `disable.native` marker file prevents
//! installation; the wrapper's body is wrapped in `catch_unwind` and its ABI is
//! `system-unwind` so a panic in the game's own function is not turned into an abort.

use std::ffi::c_void;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::OnceLock;

use crate::log;

static LAST_MESSAGE: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

/// Writes a line unless it repeats the previous one: the dialogue builder calls the
/// hooked function whenever it rebuilds, and an unchanged outcome is not news.
fn log_change(message: String) {
    let mut last = LAST_MESSAGE.lock().unwrap_or_else(|e| e.into_inner());
    if *last != message {
        log(&message);
        *last = message;
    }
}

/// Where the hook's dependencies live in one exact, reviewed game build. Builds not
/// listed here are refused: the hook never searches for its sites at startup.
pub struct Profile {
    pub game_version: &'static str,
    pub sha256: &'static str,
    timestamp: u32,
    image_size: u32,
    compute_rva: usize,
    /// (site RVA, expected bytes) for the apply and the dialogue site, in [`SITE_LABELS`] order.
    sites: [(usize, [u8; 5]); 2],
}

const SITE_LABELS: [&str; 2] = ["apply", "dialogue"];

/// Adding a build: run `LTTM2/lt_swap_re/migrate.py OLD_EXE NEW_EXE`, review the report
/// (FINDINGS.md, "Migration"), add an entry, then confirm in game with a `swap_dry_run.enabled` marker.
pub const PROFILES: [Profile; 3] = [
    // compute's code changed from 0.6.3 (765 vs 786 instructions; some scoring constants
    // gone), so only its contract is proven statically: same out-parameter (None = cap
    // u64::MAX; success writes cap, then ptr+len), apply_coach_swap_order equivalent
    // (calls at +0x45, stores at runner+0x198), dialogue site by unique context, exactly
    // two callers. The order's meaning (order[row] = draft index) rests on the in-game
    // dry-run.
    Profile {
        game_version: "0.6.4",
        sha256: "a1d05045405a12e2bed182daeafcad8c8a40cf29ab68ba6ea5bfd30257439582",
        timestamp: 0x6ACF_3112,
        image_size: 0x0533_F000,
        compute_rva: 0x20964f0,
        sites: [
            (0x208e8b5, [0xe8, 0x36, 0x7c, 0x00, 0x00]),
            (0x209f352, [0xe8, 0x99, 0x71, 0xff, 0xff]),
        ],
    },
    // compute: the 0.6.4 code with one small conversion step removed (13 structural
    // differences), same out-parameter contract; apply_coach_swap_order byte-identical
    // to 0.6.4; dialogue site by unique context; exactly two callers.
    Profile {
        game_version: "0.6.5",
        sha256: "daf040db8a3a3ae136f3ef8ce704e2718f049656b6b5e4872eacb599811d30b5",
        timestamp: 0x6AD8_5238,
        image_size: 0x0530_9000,
        compute_rva: 0x22b2b40,
        sites: [
            (0x22aaf05, [0xe8, 0x36, 0x7c, 0x00, 0x00]),
            (0x22bb972, [0xe8, 0xc9, 0x71, 0xff, 0xff]),
        ],
    },
    // Beta branch, second 0.7.0 build. compute reworked like 0.6.4's (other registers,
    // moved data offsets), same out-parameter contract; apply_coach_swap_order equal to
    // the first beta's apart from one game-data offset; dialogue site by unique context;
    // exactly two callers.
    Profile {
        game_version: "0.7.0b2",
        sha256: "79423383a353b63f4a4f907889eae2d6cc778c53519e9e08828b200c2a88e744",
        timestamp: 0x6AD8_5082,
        image_size: 0x058F_B000,
        compute_rva: 0x2e59840,
        sites: [
            (0x2e51c25, [0xe8, 0x16, 0x7c, 0x00, 0x00]),
            (0x2e62902, [0xe8, 0x39, 0x6f, 0xff, 0xff]),
        ],
    },
];

pub fn profile_for(sha256: &str) -> Option<&'static Profile> {
    PROFILES.iter().find(|p| p.sha256 == sha256)
}

// ------------------------------------------------------------------ pure logic

/// Whether a result is written or only logged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    DryRun,
    Live,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Plan {
    /// The game's order already equals LT Coach's.
    Same,
    /// Overwrite the five elements with these.
    Replace([u64; 5]),
    /// Do nothing, for this reason.
    Refuse(&'static str),
}

fn is_permutation(values: &[u64]) -> bool {
    let mut seen = [false; 5];
    values.len() == 5
        && values
            .iter()
            .all(|&v| v < 5 && !std::mem::replace(&mut seen[v as usize], true))
}

/// Decides what to do with the game's own result, given LT Coach's target.
pub fn plan(current: &[u64], target: [usize; 5]) -> Plan {
    if !is_permutation(current) {
        return Plan::Refuse("the game's own order is not a permutation of 0..5");
    }
    let target: [u64; 5] = target.map(|v| v as u64);
    if !is_permutation(&target) {
        return Plan::Refuse("the Coach's order is not a permutation of 0..5");
    }
    if current == target {
        Plan::Same
    } else {
        Plan::Replace(target)
    }
}

// ------------------------------------------------------------ Windows access

mod win {
    use std::ffi::c_void;

    #[repr(C)]
    pub struct MemoryInfo {
        pub base: usize,
        pub allocation_base: usize,
        pub allocation_protect: u32,
        pub partition: u16,
        pub region_size: usize,
        pub state: u32,
        pub protect: u32,
        pub kind: u32,
    }

    #[link(name = "kernel32")]
    extern "system" {
        pub fn GetModuleHandleW(name: *const u16) -> *mut c_void;
        pub fn GetModuleHandleExW(flags: u32, address: *const c_void, module: *mut *mut c_void) -> i32;
        pub fn GetModuleFileNameW(module: *mut c_void, name: *mut u16, size: u32) -> u32;
        pub fn VirtualAlloc(address: *mut c_void, size: usize, kind: u32, protect: u32) -> *mut c_void;
        pub fn VirtualProtect(address: *mut c_void, size: usize, protect: u32, old: *mut u32) -> i32;
        pub fn VirtualQuery(address: *const c_void, info: *mut MemoryInfo, length: usize) -> usize;
        pub fn FlushInstructionCache(process: *mut c_void, address: *const c_void, size: usize) -> i32;
        pub fn GetCurrentProcess() -> *mut c_void;
    }

    #[link(name = "bcrypt")]
    extern "system" {
        pub fn BCryptOpenAlgorithmProvider(
            handle: *mut *mut c_void,
            algorithm: *const u16,
            implementation: *const u16,
            flags: u32,
        ) -> i32;
        pub fn BCryptHash(
            handle: *mut c_void,
            secret: *const u8,
            secret_size: u32,
            input: *const u8,
            input_size: u32,
            output: *mut u8,
            output_size: u32,
        ) -> i32;
        pub fn BCryptCloseAlgorithmProvider(handle: *mut c_void, flags: u32) -> i32;
    }
}

const READABLE: u32 = 0x02 | 0x04 | 0x08 | 0x20 | 0x40 | 0x80;
const WRITABLE: u32 = 0x04 | 0x08 | 0x40 | 0x80;

fn region(address: usize, bytes: usize) -> Option<u32> {
    if address < 0x10000 || address % 8 != 0 || bytes == 0 {
        return None;
    }
    let mut info = win::MemoryInfo {
        base: 0,
        allocation_base: 0,
        allocation_protect: 0,
        partition: 0,
        region_size: 0,
        state: 0,
        protect: 0,
        kind: 0,
    };
    let got = unsafe {
        win::VirtualQuery(address as *const c_void, &mut info, std::mem::size_of::<win::MemoryInfo>())
    };
    let end = address.checked_add(bytes)?;
    // 0x101: PAGE_NOACCESS / PAGE_GUARD.
    (got != 0 && info.state == 0x1000 && info.protect & 0x101 == 0 && end <= info.base + info.region_size)
        .then_some(info.protect)
}

/// Reads `count` u64 words if the whole range is committed and readable.
fn read_words(address: usize, count: usize) -> Option<Vec<u64>> {
    if count == 0 || count > 4096 || region(address, count * 8)? & READABLE == 0 {
        return None;
    }
    Some((0..count).map(|i| unsafe { std::ptr::read_volatile((address + i * 8) as *const u64) }).collect())
}

/// Writes words if the range is committed and writable.
fn write_words(address: usize, words: &[u64]) -> bool {
    if words.is_empty() || region(address, words.len() * 8).map_or(true, |p| p & WRITABLE == 0) {
        return false;
    }
    for (i, word) in words.iter().enumerate() {
        unsafe { std::ptr::write_volatile((address + i * 8) as *mut u64, *word) };
    }
    true
}

fn exe_base() -> usize {
    unsafe { win::GetModuleHandleW(std::ptr::null()) as usize }
}

fn sha256_of_file(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read the executable: {e}"))?;
    let size = u32::try_from(bytes.len()).map_err(|_| "executable too large".to_string())?;
    let algorithm: Vec<u16> = "SHA256\0".encode_utf16().collect();
    let mut handle = std::ptr::null_mut();
    let mut digest = [0u8; 32];
    unsafe {
        if win::BCryptOpenAlgorithmProvider(&mut handle, algorithm.as_ptr(), std::ptr::null(), 0) < 0 {
            return Err("SHA256 provider unavailable".into());
        }
        let result = win::BCryptHash(handle, std::ptr::null(), 0, bytes.as_ptr(), size, digest.as_mut_ptr(), 32);
        win::BCryptCloseAlgorithmProvider(handle, 0);
        if result < 0 {
            return Err("SHA256 failed".into());
        }
    }
    Ok(digest.iter().map(|b| format!("{b:02x}")).collect())
}

fn exe_path() -> Option<PathBuf> {
    unsafe {
        let module = win::GetModuleHandleW(std::ptr::null());
        let mut buffer = vec![0u16; 32768];
        let length = win::GetModuleFileNameW(module, buffer.as_mut_ptr(), buffer.len() as u32) as usize;
        (length > 0 && length < buffer.len()).then(|| PathBuf::from(String::from_utf16_lossy(&buffer[..length])))
    }
}

// ---------------------------------------------------------------- installation

static HASH_RESULT: OnceLock<Result<String, String>> = OnceLock::new();
static ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static INSTALLED: OnceLock<()> = OnceLock::new();

/// Starts hashing the executable in the background (about a second for 87 MB).
pub fn begin_hashing() {
    static STARTED: OnceLock<()> = OnceLock::new();
    STARTED.get_or_init(|| {
        std::thread::spawn(|| {
            let result = match exe_path() {
                Some(path) => sha256_of_file(&path),
                None => Err("executable path unavailable".to_string()),
            };
            match &result {
                Ok(sha) => log(&format!(
                    "swap: executable SHA-256 {sha} ({})",
                    match profile_for(sha) {
                        Some(profile) => format!("game {}, a reviewed build for the swap hook", profile.game_version),
                        None => "not a reviewed build: the swap hook stays off".to_string(),
                    }
                )),
                Err(error) => log(&format!("swap: hashing failed: {error}")),
            }
            let _ = HASH_RESULT.set(result);
        });
    });
}

/// `None` while the hash is still being computed.
pub fn hash_result() -> Option<&'static Result<String, String>> {
    HASH_RESULT.get()
}

pub fn is_installed() -> bool {
    INSTALLED.get().is_some()
}

fn relative_call(site: usize, destination: usize) -> Result<[u8; 5], String> {
    let relative = i32::try_from(destination as i128 - (site as i128 + 5))
        .map_err(|_| "relay outside CALL rel32 range".to_string())?;
    let mut bytes = [0xe8, 0, 0, 0, 0];
    bytes[1..].copy_from_slice(&relative.to_le_bytes());
    Ok(bytes)
}

/// A 14-byte `jmp [rip+0]; <absolute address>` relay within rel32 reach of `site`.
/// Leaked on purpose: it lives until the process exits.
unsafe fn make_relay(site: usize, destination: usize) -> Result<usize, String> {
    let anchor = site & !0xffff;
    for distance in (0x10000..0x7000_0000usize).step_by(0x10000) {
        for address in [anchor.checked_sub(distance), anchor.checked_add(distance)].into_iter().flatten() {
            if address < 0x10000 {
                continue;
            }
            let block = win::VirtualAlloc(address as *mut c_void, 0x1000, 0x3000, 0x04);
            if block.is_null() {
                continue;
            }
            let mut code = [0u8; 14];
            code[..6].copy_from_slice(&[0xff, 0x25, 0, 0, 0, 0]);
            code[6..].copy_from_slice(&destination.to_le_bytes());
            std::ptr::copy_nonoverlapping(code.as_ptr(), block.cast::<u8>(), code.len());
            let mut old = 0;
            if win::VirtualProtect(block, 0x1000, 0x20, &mut old) == 0 {
                return Err("cannot make the relay executable".into());
            }
            if win::FlushInstructionCache(win::GetCurrentProcess(), block, code.len()) == 0 {
                return Err("cannot flush the relay".into());
            }
            relative_call(site, block as usize)?;
            return Ok(block as usize);
        }
    }
    Err("cannot allocate a relay within reach".into())
}

unsafe fn read_bytes<const N: usize>(address: usize) -> [u8; N] {
    let mut out = [0u8; N];
    std::ptr::copy_nonoverlapping(address as *const u8, out.as_mut_ptr(), N);
    out
}

unsafe fn write_site(address: usize, bytes: [u8; 5]) -> Result<(), String> {
    let mut old = 0;
    if win::VirtualProtect(address as *mut c_void, 5, 0x40, &mut old) == 0 {
        return Err("cannot make the call site writable".into());
    }
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), address as *mut u8, 5);
    let flushed = win::FlushInstructionCache(win::GetCurrentProcess(), address as *const c_void, 5) != 0;
    let mut ignored = 0;
    let restored = win::VirtualProtect(address as *mut c_void, 5, old, &mut ignored) != 0;
    if !flushed || !restored {
        return Err("call site written but flush/protect restore failed".into());
    }
    Ok(())
}

/// Verifies the build and both call sites, then redirects them. Call only on the
/// title screen, after [`hash_result`] is `Some`.
pub fn install(log_dir: Option<&Path>) -> Result<(), String> {
    if log_dir.is_some_and(|d| d.join("disable.native").exists()) {
        return Err("disable.native is present".into());
    }
    let sha = hash_result().ok_or("executable not hashed yet")?.clone()?;
    let profile = profile_for(&sha)
        .ok_or_else(|| format!("executable SHA-256 {sha} is not a reviewed build; nothing patched"))?;
    let base = exe_base();
    if base == 0 {
        return Err("executable module unavailable".into());
    }
    let (timestamp, image_size) = unsafe {
        let e_lfanew = std::ptr::read_unaligned((base + 0x3c) as *const u32) as usize;
        let nt = base + e_lfanew;
        (
            std::ptr::read_unaligned((nt + 8) as *const u32),
            std::ptr::read_unaligned((nt + 24 + 56) as *const u32),
        )
    };
    if timestamp != profile.timestamp || image_size != profile.image_size {
        return Err(format!("PE timestamp {timestamp:#x} / image size {image_size:#x} differ"));
    }

    let compute_rva = profile.compute_rva;
    let sites = profile.sites;
    let original = base + compute_rva;
    for ((rva, expected), label) in sites.into_iter().zip(SITE_LABELS) {
        let site = base + rva;
        let actual = unsafe { read_bytes::<5>(site) };
        if actual != expected {
            return Err(format!("{label} site {rva:#x} holds {actual:02x?}, expected {expected:02x?}"));
        }
        let relative = i32::from_le_bytes([actual[1], actual[2], actual[3], actual[4]]);
        let target = (site as i64 + 5 + relative as i64) as usize;
        if target != original {
            return Err(format!("{label} site {rva:#x} calls {:#x}, expected {compute_rva:#x}", target.wrapping_sub(base)));
        }
    }

    ORIGINAL.store(original, Ordering::Release);
    let hooks: [usize; 2] = [hook_apply as *const () as usize, hook_dialogue as *const () as usize];
    let mut relays = [0usize; 2];
    let mut patched = [[0u8; 5]; 2];
    unsafe {
        for (i, (rva, _)) in sites.iter().enumerate() {
            let site = base + rva;
            relays[i] = make_relay(site, hooks[i])?;
            patched[i] = relative_call(site, relays[i])?;
        }
        let mut module = std::ptr::null_mut();
        if win::GetModuleHandleExW(1 | 4, hook_apply as *const c_void, &mut module) == 0 {
            return Err("cannot pin the DLL; nothing patched".into());
        }
        for (i, ((rva, _), label)) in sites.iter().zip(SITE_LABELS).enumerate() {
            let site = base + rva;
            let revert = |upto: usize| {
                for (rva_j, expected_j) in sites.iter().take(upto + 1) {
                    let _ = write_site(base + rva_j, *expected_j);
                }
            };
            if let Err(error) = write_site(site, patched[i]) {
                revert(i);
                return Err(format!("{label}: {error}; reverted"));
            }
            let readback = read_bytes::<5>(site);
            if readback != patched[i] {
                revert(i);
                return Err(format!("{label}: read-back {readback:02x?} differs; reverted"));
            }
        }
    }
    let _ = INSTALLED.set(());
    log(&format!(
        "swap: PATCHED game {} base={base:#x} original={original:#x} sites=[{:#x}, {:#x}]",
        profile.game_version,
        base + sites[0].0,
        base + sites[1].0
    ));
    Ok(())
}

// --------------------------------------------------------------------- wrappers

/// "system-unwind": if the game's function panics, the unwind passes through this
/// frame instead of aborting the process.
type ComputeFn = unsafe extern "system-unwind" fn(usize, usize) -> usize;

unsafe extern "system-unwind" fn hook_apply(out: usize, this: usize) -> usize {
    wrapper(0, out, this)
}

unsafe extern "system-unwind" fn hook_dialogue(out: usize, this: usize) -> usize {
    wrapper(1, out, this)
}

/// Calls the original first, with the caller's own arguments, then maybe rewrites.
unsafe fn wrapper(site: usize, out: usize, this: usize) -> usize {
    let original: ComputeFn = std::mem::transmute(ORIGINAL.load(Ordering::Acquire));
    let result = original(out, this);
    let _ = catch_unwind(AssertUnwindSafe(|| after(site, out, this)));
    result
}

static LAYOUT_WARNED: AtomicU32 = AtomicU32::new(0);

/// Reads the returned `Option<Vec<usize>>` and, if the Coach's target differs,
/// overwrites the five elements in place.
fn after(site: usize, out: usize, this: usize) {
    let Some(words) = read_words(out, 3) else { return };
    if words[0] == u64::MAX {
        return; // None
    }
    // Layout (cap, ptr, len), as observed: [5, <heap>, 5]. Anything else is left alone.
    let (cap, ptr, len) = (words[0], words[1], words[2]);
    if len != 5 || cap < 5 || ptr < 0x10000 || ptr % 8 != 0 {
        if LAYOUT_WARNED.fetch_add(1, Ordering::Relaxed) < 3 {
            log(&format!("swap: unexpected result layout {words:#x?}; left alone"));
        }
        return;
    }
    let Some(current) = read_words(ptr as usize, 5) else { return };
    let Some((target, mode)) = crate::swap_target(site, &current, this) else { return };
    match plan(&current, target) {
        Plan::Same => log_change(format!(
            "swap: {} call: the game's order {current:?} already equals LT Coach's",
            SITE_LABELS[site]
        )),
        Plan::Refuse(reason) => log_change(format!("swap: refused: {reason}")),
        Plan::Replace(order) => match mode {
            Mode::DryRun => log_change(format!(
                "swap: DRY RUN {} call: would replace {current:?} with {order:?} (not written: swap_dry_run.enabled is present)",
                SITE_LABELS[site]
            )),
            Mode::Live => {
                let wrote = write_words(ptr as usize, &order);
                let readback = read_words(ptr as usize, 5);
                log_change(format!(
                    "swap: LIVE {} call: replaced {current:?} with {order:?} (wrote={wrote}, read-back={readback:?})",
                    SITE_LABELS[site]
                ));
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_every_profile_both_call_sites_resolve_to_the_compute_function() {
        for profile in &PROFILES {
            for ((rva, bytes), label) in profile.sites.into_iter().zip(SITE_LABELS) {
                let at = format!("{} {label}", profile.game_version);
                assert_eq!(bytes[0], 0xe8, "{at}");
                let relative = i32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]);
                assert_eq!((rva as i64 + 5 + relative as i64) as usize, profile.compute_rva, "{at}");
            }
        }
    }

    #[test]
    fn profiles_are_distinct_and_found_only_by_their_exact_hash() {
        for (i, a) in PROFILES.iter().enumerate() {
            assert_eq!(a.sha256.len(), 64);
            assert!(a.sha256.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)));
            assert_eq!(profile_for(a.sha256).map(|p| p.game_version), Some(a.game_version));
            for b in &PROFILES[i + 1..] {
                assert_ne!(a.sha256, b.sha256);
                assert_ne!(a.game_version, b.game_version);
            }
        }
        assert!(profile_for("").is_none());
        assert!(profile_for(&PROFILES[0].sha256.to_uppercase()).is_none());
    }

    /// Only on the developer's machine: each profile against the preserved executable
    /// it was reviewed on.
    #[test]
    fn profiles_match_the_preserved_executables_when_available() {
        let builds = Path::new("D:/LTTM2/LTDirectControl/research/game-builds");
        for profile in &PROFILES {
            let Ok(exe) = std::fs::read(builds.join(profile.game_version).join("TeamfightManager2.exe")) else {
                continue;
            };
            let u16_at = |o: usize| usize::from(u16::from_le_bytes([exe[o], exe[o + 1]]));
            let u32_at = |o: usize| u32::from_le_bytes([exe[o], exe[o + 1], exe[o + 2], exe[o + 3]]);
            let nt = u32_at(0x3c) as usize;
            assert_eq!(u32_at(nt + 8), profile.timestamp, "{}", profile.game_version);
            assert_eq!(u32_at(nt + 24 + 56), profile.image_size, "{}", profile.game_version);
            let sections = nt + 24 + u16_at(nt + 20);
            let file_offset = |rva: usize| {
                (0..u16_at(nt + 6))
                    .map(|i| sections + i * 40)
                    .find_map(|s| {
                        let (size, va, raw) = (u32_at(s + 8) as usize, u32_at(s + 12) as usize, u32_at(s + 20) as usize);
                        (va <= rva && rva < va + size).then(|| rva - va + raw)
                    })
                    .expect("RVA inside a section")
            };
            for ((rva, bytes), label) in profile.sites.into_iter().zip(SITE_LABELS) {
                let at = file_offset(rva);
                assert_eq!(exe[at..at + 5], bytes, "{} {label}", profile.game_version);
            }
        }
    }

    #[test]
    fn relative_calls_encode() {
        assert_eq!(relative_call(0x1000, 0x2000).unwrap(), [0xe8, 0xfb, 0x0f, 0x00, 0x00]);
        assert!(relative_call(0, 0x1_0000_0000).is_err());
    }

    #[test]
    fn a_different_target_is_replaced() {
        assert_eq!(plan(&[2, 1, 0, 3, 4], [0, 1, 2, 3, 4]), Plan::Replace([0, 1, 2, 3, 4]));
        assert_eq!(plan(&[0, 1, 2, 3, 4], [4, 3, 2, 1, 0]), Plan::Replace([4, 3, 2, 1, 0]));
    }

    #[test]
    fn an_identical_target_changes_nothing() {
        assert_eq!(plan(&[2, 1, 0, 3, 4], [2, 1, 0, 3, 4]), Plan::Same);
    }

    #[test]
    fn nothing_is_written_over_a_result_that_is_not_a_permutation() {
        assert!(matches!(plan(&[2, 1, 0, 3], [0, 1, 2, 3, 4]), Plan::Refuse(_)));
        assert!(matches!(plan(&[2, 1, 1, 3, 4], [0, 1, 2, 3, 4]), Plan::Refuse(_)));
        assert!(matches!(plan(&[2, 1, 0, 3, 9], [0, 1, 2, 3, 4]), Plan::Refuse(_)));
    }

    #[test]
    fn a_target_that_is_not_a_permutation_is_refused() {
        assert!(matches!(plan(&[0, 1, 2, 3, 4], [0, 1, 2, 3, 3]), Plan::Refuse(_)));
        assert!(matches!(plan(&[0, 1, 2, 3, 4], [0, 1, 2, 3, 5]), Plan::Refuse(_)));
    }

    #[test]
    fn memory_helpers_refuse_bad_addresses_and_write_only_writable_memory() {
        assert!(read_words(0, 1).is_none());
        assert!(read_words(0x1234_5678, 1).is_none());
        let mut buffer = vec![0u64, 1, 2, 3, 4];
        assert!(write_words(buffer.as_mut_ptr() as usize, &[4, 3, 2, 1, 0]));
        assert_eq!(buffer, vec![4, 3, 2, 1, 0]);
        assert_eq!(read_words(buffer.as_ptr() as usize, 5), Some(vec![4, 3, 2, 1, 0]));
    }
}
