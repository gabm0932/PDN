#!/usr/bin/env python3
"""
Dice-Roll Monkey Tester (Stochastic State Machine) for podnext.
Simulates a random user by rolling virtual dice (1-6) to pick commands,
arguments, flags, container targets, and payloads across state transitions.
"""

import os
import sys
import time
import json
import random
import shutil
import subprocess
from pathlib import Path
from typing import List, Dict, Any, Tuple, Optional

HOME = Path(os.environ.get("HOME", "/data/data/com.termux/files/home"))
TMPDIR = Path(os.environ.get("TMPDIR", "/data/data/com.termux/files/usr/tmp"))
SCRATCH_DIR = TMPDIR / "podnext_monkey_scratch"
PODNEXT_BIN = Path("/data/data/com.termux/files/usr/bin/podnext")
LOCK_DIR_PN_NEXT = HOME / ".pn-next" / "locks"
LOCK_DIR_PODNEXT = HOME / ".podnext" / "locks"
SESSIONS_DIR = HOME / ".podnext" / "sessions"

VALID_TARGETS = ["deb-pdn", "ubuntu", "alpine"]
ALL_TARGETS = ["deb-pdn", "ubuntu", "alpine", "nonexistent-target", "deb-renamed"]

COMMANDS = [
    "list",
    "run",
    "login",
    "ps",
    "copy",
    "sync",
    "rename",
    "reset",
    "build",
    "search",
]


class VirtualDice:
    """Simulates rolling virtual 6-sided dice (d6)."""
    def __init__(self, seed: int):
        self.seed = seed
        self.rng = random.Random(seed)
        self.roll_count = 0

    def roll(self, sides: int = 6) -> int:
        self.roll_count += 1
        return self.rng.randint(1, sides)

    def roll_2d6(self) -> Tuple[int, int]:
        return self.roll(6), self.roll(6)


class MonkeyTester:
    def __init__(self, seed: int, total_steps: int = 35, output_path: Optional[Path] = None):
        self.seed = seed
        self.total_steps = total_steps
        self.dice = VirtualDice(seed)
        self.output_path = output_path or (HOME / "workspace" / "PDN" / "scripts" / "dice_monkey_report.json")
        self.current_state = "IDLE"
        self.steps_record: List[Dict[str, Any]] = []
        self.anomalies: List[Dict[str, Any]] = []
        self.renamed_map: Dict[str, str] = {}  # original -> current name

    def setup(self):
        SCRATCH_DIR.mkdir(parents=True, exist_ok=True)
        # Clean old payload files
        for f in SCRATCH_DIR.glob("*"):
            if f.is_dir():
                shutil.rmtree(f, ignore_errors=True)
            else:
                f.unlink(missing_ok=True)

    def cleanup(self):
        # Restore any renamed container
        for orig, current in list(self.renamed_map.items()):
            if current != orig:
                print(f"[CLEANUP] Restoring container rename: {current} -> {orig}")
                try:
                    subprocess.run(
                        [str(PODNEXT_BIN), "rename", current, orig],
                        capture_output=True,
                        text=True,
                        timeout=10,
                    )
                except Exception as e:
                    print(f"[CLEANUP] Failed to restore {current} -> {orig}: {e}")
        # Remove scratch dir
        shutil.rmtree(SCRATCH_DIR, ignore_errors=True)

    def pick_target(self) -> str:
        d = self.dice.roll(6)
        if d == 1:
            return self.renamed_map.get("deb-pdn", "deb-pdn")
        elif d == 2:
            return self.renamed_map.get("ubuntu", "ubuntu")
        elif d == 3:
            return self.renamed_map.get("alpine", "alpine")
        elif d == 4:
            return self.renamed_map.get("deb-renamed", "deb-renamed")
        elif d == 5:
            return "ghost-container-404"
        else:
            return "inv@lid_name#!"

    def create_random_payload(self) -> Tuple[Path, str]:
        p_type = self.dice.roll(6)
        payload_name = f"payload_{self.dice.roll_count}_{time.time_ns()}"
        if p_type in (1, 2):
            # Single text file
            file_path = SCRATCH_DIR / f"{payload_name}.txt"
            content = f"Monkey test data {payload_name}\nTimestamp: {time.time()}\nRandom: {self.dice.roll(1000)}"
            file_path.write_text(content)
            return file_path, "file"
        elif p_type in (3, 4):
            # Directory with multiple files
            dir_path = SCRATCH_DIR / f"{payload_name}_dir"
            dir_path.mkdir(parents=True, exist_ok=True)
            for i in range(1, 4):
                sub = dir_path / f"sub_{i}.dat"
                sub.write_text(f"Payload block {i} in {payload_name}\n")
            return dir_path, "dir"
        elif p_type == 5:
            # Empty file
            file_path = SCRATCH_DIR / f"{payload_name}_empty.dat"
            file_path.touch()
            return file_path, "empty_file"
        else:
            # Binary payload
            file_path = SCRATCH_DIR / f"{payload_name}.bin"
            file_path.write_bytes(os.urandom(256))
            return file_path, "binary_file"

    def generate_command(self, step: int) -> Tuple[List[str], Optional[bytes], Dict[str, Any]]:
        d1, d2 = self.dice.roll_2d6()
        metadata: Dict[str, Any] = {"d1": d1, "d2": d2, "step": step}

        # Select command based on virtual dice rolls (1-6)
        if d1 == 1:
            cmd_name = "list" if d2 <= 3 else "ps"
        elif d1 == 2:
            cmd_name = "run"
        elif d1 == 3:
            cmd_name = "login"
        elif d1 == 4:
            cmd_name = "copy" if d2 <= 3 else "sync"
        elif d1 == 5:
            cmd_name = "rename" if d2 <= 3 else "reset"
        elif d1 == 6:
            cmd_name = "build" if d2 <= 3 else "search"
        else:
            cmd_name = "list"

        metadata["selected_command"] = cmd_name
        args: List[str] = [str(PODNEXT_BIN), cmd_name]
        stdin_data: Optional[bytes] = None

        if cmd_name == "list":
            d_flag = self.dice.roll(6)
            metadata["flag_roll"] = d_flag
            if d_flag == 1:
                pass
            elif d_flag == 2:
                args.extend(["-f", "json"])
            elif d_flag == 3:
                args.extend(["-f", "yaml"])
            elif d_flag == 4:
                args.extend(["-f", "text"])
            elif d_flag == 5:
                args.append("--no-all")
            else:
                args.append("-a")

        elif cmd_name == "ps":
            d_flag = self.dice.roll(6)
            metadata["flag_roll"] = d_flag
            if d_flag == 1:
                pass
            elif d_flag == 2:
                args.extend(["-f", "json"])
            elif d_flag == 3:
                args.extend(["-f", "yaml"])
            elif d_flag == 4:
                args.extend(["-f", "text"])
            elif d_flag == 5:
                args.append("-v")
            else:
                args.append("--no-sysvipc")

        elif cmd_name == "run":
            target = self.pick_target()
            args.append(target)
            metadata["target"] = target
            
            d_flag = self.dice.roll(6)
            metadata["flag_roll"] = d_flag
            if d_flag == 2:
                args.append("--isolated")
            elif d_flag == 3:
                args.extend(["-e", f"MONKEY_SEED={self.seed}"])
            elif d_flag == 4:
                args.extend(["--workdir", "/tmp"])
            elif d_flag == 5:
                args.extend(["-u", "0:0"])

            args.append("--")
            d_sub = self.dice.roll(6)
            metadata["subcmd_roll"] = d_sub
            if d_sub == 1:
                args.extend(["echo", f"monkey_step_{step}"])
            elif d_sub == 2:
                args.extend(["uname", "-a"])
            elif d_sub == 3:
                args.extend(["id"])
            elif d_sub == 4:
                args.extend(["ls", "-la", "/tmp"])
            elif d_sub == 5:
                args.extend(["cat", "/etc/os-release"])
            else:
                args.extend(["/bin/sh", "-c", "exit 42"])

        elif cmd_name == "login":
            target = self.pick_target()
            metadata["target"] = target
            args.append(target)
            
            d_mode = self.dice.roll(6)
            metadata["mode_roll"] = d_mode
            if d_mode in (1, 2):
                stdin_data = b"echo monkey_login_stdin; exit\n"
            elif d_mode in (3, 4):
                stdin_data = b"whoami; exit\n"
            elif d_mode == 5:
                args.extend(["--", "/bin/sh", "-c", "echo login_exec_mode"])
            else:
                args.append("--isolated")
                stdin_data = b"exit\n"

        elif cmd_name == "copy":
            payload_path, p_type = self.create_random_payload()
            target = self.pick_target()
            metadata["target"] = target
            metadata["payload"] = str(payload_path)
            metadata["payload_type"] = p_type

            d_dir = self.dice.roll(6)
            metadata["dir_roll"] = d_dir
            if d_dir in (1, 2):
                # Host to container
                if p_type == "dir":
                    args.append("-R")
                args.extend([str(payload_path), f"{target}:/tmp/{payload_path.name}"])
            elif d_dir in (3, 4):
                # Container to host
                dest_file = SCRATCH_DIR / f"fetched_{payload_path.name}"
                args.extend([f"{target}:/etc/hosts", str(dest_file)])
            elif d_dir == 5:
                # Copy with move flag (-m)
                single_f = SCRATCH_DIR / f"move_src_{step}.txt"
                single_f.write_text("move test content\n")
                args.extend(["-m", str(single_f), f"{target}:/tmp/moved_{step}.txt"])
            else:
                # Malformed / Traversal test
                args.extend([str(payload_path), f"{target}:/../../escape_test"])

        elif cmd_name == "sync":
            payload_path, p_type = self.create_random_payload()
            target = self.pick_target()
            metadata["target"] = target
            metadata["payload"] = str(payload_path)
            
            d_mode = self.dice.roll(6)
            metadata["mode_roll"] = d_mode
            if d_mode in (1, 2):
                args.extend([str(payload_path), f"{target}:/tmp/{payload_path.name}"])
            elif d_mode in (3, 4):
                args.extend(["--delete", str(payload_path), f"{target}:/tmp/{payload_path.name}"])
            elif d_mode == 5:
                # Non-existent source
                args.extend(["/nonexistent_src_dir_404", f"{target}:/tmp/dummy"])
            else:
                # Container to host
                dst_local = SCRATCH_DIR / f"sync_out_{step}"
                dst_local.mkdir(exist_ok=True)
                args.extend([f"{target}:/etc", str(dst_local)])

        elif cmd_name == "rename":
            d_action = self.dice.roll(6)
            metadata["action_roll"] = d_action
            if d_action in (1, 2, 3):
                # Safe mutation on deb-renamed
                cur_name = self.renamed_map.get("deb-renamed", "deb-renamed")
                new_name = "deb-renamed-tmp" if cur_name == "deb-renamed" else "deb-renamed"
                args.extend([cur_name, new_name])
                metadata["rename_transition"] = (cur_name, new_name)
            elif d_action == 4:
                # Rename container to itself (expected failure)
                target = self.pick_target()
                args.extend([target, target])
            elif d_action == 5:
                # Rename non-existent container
                args.extend(["ghost_container_xyz", "new_ghost_name"])
            else:
                # Rename with invalid characters
                args.extend(["deb-renamed", "inv@lid::name!!"])

        elif cmd_name == "reset":
            d_action = self.dice.roll(6)
            metadata["action_roll"] = d_action
            if d_action in (1, 2):
                # Reset non-existent container
                args.append("nonexistent-box-404")
            elif d_action == 3:
                # Reset with empty argument
                args.append("")
            elif d_action == 4:
                # Help flag
                args.append("--help")
            elif d_action == 5:
                # Non-existent with isolated flag
                args.extend(["--isolated", "ghost_box"])
            else:
                # Invalid flag
                args.append("--unknown-monkey-flag")

        elif cmd_name == "build":
            d_sub = self.dice.roll(6)
            metadata["sub_roll"] = d_sub
            if d_sub == 1:
                args.append("list")
            elif d_sub == 2:
                args.extend(["status", f"build-{step}"])
            elif d_sub == 3:
                args.extend(["artifacts", f"build-{step}"])
            elif d_sub == 4:
                args.extend(["logs", f"build-{step}"])
            elif d_sub == 5:
                args.extend(["cancel", f"build-{step}"])
            else:
                args.extend(["local", "/nonexistent-dockerfile-dir"])

        elif cmd_name == "search":
            d_sub = self.dice.roll(6)
            metadata["sub_roll"] = d_sub
            if d_sub == 1:
                pass
            elif d_sub == 2:
                args.extend(["alpine", "--limit", "3"])
            elif d_sub == 3:
                args.extend(["debian", "-f", "json", "-l", "2"])
            elif d_sub == 4:
                args.extend(["ubuntu", "-l", "1"])
            elif d_sub == 5:
                args.extend(["nonexistent_image_foo_bar_xyz"])
            else:
                args.extend(["--format", "yaml"])

        return args, stdin_data, metadata

    def check_invariants(self, step: int, cmd_args: List[str], exit_code: int, stdout: str, stderr: str) -> Dict[str, Any]:
        invariant_status = {
            "crashed": False,
            "crash_detail": None,
            "leftover_locks": [],
            "stale_sessions": [],
            "state_desynced": False,
            "desync_detail": None,
        }

        # 1. Crash check (fatal signals, panics)
        if exit_code in (132, 133, 134, 139):
            invariant_status["crashed"] = True
            invariant_status["crash_detail"] = f"Fatal exit code {exit_code} (signal {exit_code - 128})"
        combined = stdout + stderr
        if "panic" in combined.lower() or "stack backtrace:" in combined.lower() or "segmentation fault" in combined.lower():
            invariant_status["crashed"] = True
            invariant_status["crash_detail"] = "Rust panic or segfault detected in output stream"

        # 2. Leftover lockfiles
        for lock_dir in (LOCK_DIR_PN_NEXT, LOCK_DIR_PODNEXT):
            if lock_dir.exists():
                for lock_f in lock_dir.glob("*.lock"):
                    invariant_status["leftover_locks"].append(str(lock_f))

        # 3. Session desync / Stale session check
        if SESSIONS_DIR.exists():
            for sess_f in SESSIONS_DIR.glob("*.json"):
                pid_str = sess_f.stem
                if pid_str.isdigit():
                    pid = int(pid_str)
                    # Check if pid is still alive
                    try:
                        os.kill(pid, 0)
                    except OSError:
                        # Process dead but session file left behind!
                        invariant_status["stale_sessions"].append({
                            "session_file": str(sess_f),
                            "dead_pid": pid,
                        })

        # 4. State desynchronization check via podnext list
        try:
            p = subprocess.run(
                [str(PODNEXT_BIN), "list", "-f", "json"],
                capture_output=True,
                text=True,
                timeout=10,
            )
            if p.returncode != 0:
                invariant_status["state_desynced"] = True
                invariant_status["desync_detail"] = f"podnext list failed with exit code {p.returncode}: {p.stderr}"
            else:
                try:
                    data = json.loads(p.stdout)
                    # Verify basic sanity of container entries
                    for entry in data:
                        if not isinstance(entry, dict) or "name" not in entry or "status" not in entry:
                            invariant_status["state_desynced"] = True
                            invariant_status["desync_detail"] = "Malformed container JSON entry"
                            break
                except json.JSONDecodeError as jde:
                    invariant_status["state_desynced"] = True
                    invariant_status["desync_detail"] = f"JSON decode error from podnext list: {jde}"
        except Exception as e:
            invariant_status["state_desynced"] = True
            invariant_status["desync_detail"] = f"Exception querying state: {e}"

        # If any violation, record as anomaly
        if (
            invariant_status["crashed"]
            or invariant_status["leftover_locks"]
            or invariant_status["stale_sessions"]
            or invariant_status["state_desynced"]
        ):
            anomaly_record = {
                "step": step,
                "command": " ".join(cmd_args),
                "invariants": invariant_status,
            }
            self.anomalies.append(anomaly_record)

        return invariant_status

    def run(self):
        print(f"=== PODNEXT DICE-ROLL MONKEY TESTER ===")
        print(f"Random Seed: {self.seed}")
        print(f"Target Steps: {self.total_steps}")
        print(f"Base Containers: {VALID_TARGETS}")
        print(f"Commands Pool: {COMMANDS}")
        print("=======================================\n")

        self.setup()
        start_time = time.time()

        for step in range(1, self.total_steps + 1):
            args, stdin_data, meta = self.generate_command(step)
            cmd_str = " ".join(args)
            d1, d2 = meta["d1"], meta["d2"]
            cmd_name = meta["selected_command"]

            print(f"[Step {step:02d}/{self.total_steps:02d}] Dice: (d1={d1}, d2={d2}) -> CMD: {cmd_name}")
            print(f"  EXEC: {cmd_str}")

            step_start = time.time()
            try:
                proc = subprocess.run(
                    args,
                    input=stdin_data,
                    capture_output=True,
                    text=False,
                    timeout=20,
                )
                duration = time.time() - step_start
                stdout_str = proc.stdout.decode("utf-8", errors="replace")
                stderr_str = proc.stderr.decode("utf-8", errors="replace")
                exit_code = proc.returncode

                # Track successful rename transition
                if cmd_name == "rename" and exit_code == 0 and "rename_transition" in meta:
                    old_c, new_c = meta["rename_transition"]
                    self.renamed_map["deb-renamed"] = new_c
                    print(f"  [STATE UPDATE] Tracked rename {old_c} -> {new_c}")

            except subprocess.TimeoutExpired as te:
                duration = time.time() - step_start
                exit_code = -999
                stdout_str = te.stdout.decode("utf-8", errors="replace") if te.stdout else ""
                stderr_str = f"TIMED OUT after 20s: {te}"
            except Exception as e:
                duration = time.time() - step_start
                exit_code = -998
                stdout_str = ""
                stderr_str = f"Execution exception: {e}"

            # Verify invariants
            inv = self.check_invariants(step, args, exit_code, stdout_str, stderr_str)

            # Format concise output
            out_sample = stdout_str.strip().splitlines()
            out_preview = out_sample[0][:80] if out_sample else ""
            err_sample = stderr_str.strip().splitlines()
            err_preview = err_sample[0][:80] if err_sample else ""

            status_str = f"Exit: {exit_code} ({duration:.2f}s)"
            if inv["crashed"]:
                status_str += " [CRASH!]"
            if inv["leftover_locks"]:
                status_str += f" [LOCKS: {len(inv['leftover_locks'])}]"
            if inv["stale_sessions"]:
                status_str += f" [STALE SESSIONS: {len(inv['stale_sessions'])}]"
            if inv["state_desynced"]:
                status_str += " [DESYNC!]"

            print(f"  RES: {status_str}")
            if out_preview:
                print(f"  OUT: {out_preview}")
            if err_preview:
                print(f"  ERR: {err_preview}")

            self.steps_record.append({
                "step": step,
                "dice": {"d1": d1, "d2": d2},
                "command": cmd_name,
                "cmdline": args,
                "exit_code": exit_code,
                "duration_seconds": round(duration, 3),
                "stdout_sample": out_preview,
                "stderr_sample": err_preview,
                "stdout_full": stdout_str,
                "stderr_full": stderr_str,
                "invariants": inv,
            })
            print()

        total_duration = time.time() - start_time
        self.cleanup()

        # Build final report
        report = {
            "seed": self.seed,
            "total_steps": self.total_steps,
            "total_duration_seconds": round(total_duration, 2),
            "command_counts": {},
            "exit_code_counts": {},
            "anomalies_count": len(self.anomalies),
            "anomalies": self.anomalies,
            "steps": self.steps_record,
        }

        for rec in self.steps_record:
            cmd = rec["command"]
            report["command_counts"][cmd] = report["command_counts"].get(cmd, 0) + 1
            code = rec["exit_code"]
            report["exit_code_counts"][str(code)] = report["exit_code_counts"].get(str(code), 0) + 1

        self.output_path.parent.mkdir(parents=True, exist_ok=True)
        self.output_path.write_text(json.dumps(report, indent=2))
        print(f"[REPORT] Saved full empirical test run log to: {self.output_path}")

        print("\n=== SUMMARY ===")
        print(f"Executed Steps: {self.total_steps}")
        print(f"Command Distribution: {report['command_counts']}")
        print(f"Exit Code Distribution: {report['exit_code_counts']}")
        print(f"Total Anomalies / Desyncs / Crashes: {len(self.anomalies)}")
        if self.anomalies:
            print("[ANOMALIES DETECTED]:")
            for a in self.anomalies:
                print(f"  Step {a['step']}: {a['command']} -> {a['invariants']}")
        else:
            print("Zero unhandled crashes, zero leftover lockfiles, zero state desynchronizations observed.")


if __name__ == "__main__":
    import argparse
    parser = argparse.ArgumentParser(description="Podnext Stochastic Dice-Roll Monkey Tester")
    parser.add_argument("--seed", type=int, default=None, help="Random seed")
    parser.add_argument("--steps", type=int, default=35, help="Number of steps (min 30)")
    parser.add_argument("--output", type=str, default=None, help="Output JSON path")
    args = parser.parse_args()

    chosen_seed = args.seed if args.seed is not None else int(time.time())
    out_file = Path(args.output) if args.output else None

    tester = MonkeyTester(seed=chosen_seed, total_steps=args.steps, output_path=out_file)
    tester.run()
