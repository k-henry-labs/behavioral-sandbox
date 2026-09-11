"""The two rules the whole contract rests on.

An environment value goes out to the guest but never comes back in a record, and a guest command
exiting non-zero is a record and not an exception.

These run against the REAL core. There is no binary to stand in for any more -- the SDK calls
``execute_sandbox`` in this process, so a stub would be testing itself. ``dry_run`` settles a
posture without booting, which is what lets the contract be checked on a machine with no
hypervisor.
"""

import tempfile

import pytest

from boxdesk import Boxdesk

SECRET = "s3cret-value-nobody-should-see"


@pytest.fixture
def boxdesk():
    return Boxdesk()


@pytest.fixture
def root():
    """A guest root that exists, because resolution refuses one that does not. What is under test
    here is the posture, not where a tree lives."""
    return tempfile.gettempdir()


class TestEnvironmentAsymmetry:
    """A value goes out whole; only the NAME comes back."""

    def test_only_names_come_back(self, boxdesk, root):
        run = boxdesk.dry_run(
            ["env"], root=root, env={"API_KEY": SECRET, "DEBUG": "1"}
        )
        assert run.posture.env == ["API_KEY", "DEBUG"]

    def test_no_field_of_the_record_holds_the_value(self, boxdesk, root):
        run = boxdesk.dry_run(["env"], root=root, env={"API_KEY": SECRET})
        # Every readable attribute, RECURSIVELY: a field added later that carried the value
        # would slip past an assertion listing today's.
        #
        # It must recurse. The first version of this walked `dir(run)` and called `repr` on each
        # attribute, which renders a nested PyPosture as "<builtins.PyPosture object at 0x...>"
        # and hides the very field the secret lands in. Breaking the cut in `posture_of` left it
        # green, which is how that was found.
        assert SECRET not in _flatten(run), _flatten(run)

    def test_a_value_holding_an_equals_sign_keeps_its_name(self, boxdesk, root):
        run = boxdesk.dry_run(["env"], root=root, env={"TOKEN": "a=b=c"})
        assert run.posture.env == ["TOKEN"]


class TestPosture:
    def test_it_is_the_one_that_was_asked_for(self, boxdesk, root):
        run = boxdesk.dry_run(
            ["true"],
            root=root,
            vcpus=2,
            mem_mib=1024,
            net="tsi",
            rootfs="writable",
            mounts={"/mnt": root},
            shares={"tag": root},
        )
        assert run.posture.vcpus == 2
        assert run.posture.mem_mib == 1024
        assert run.posture.network == "tsi"
        assert run.posture.rootfs == "writable"
        assert run.posture.mounts == [("/mnt", root)]
        assert run.posture.shares == [("tag", root)]

    def test_the_defaults_are_the_clis_own(self, boxdesk, root):
        """Unset means the CLI's default. This SDK adds none of its own, and one that drifted
        would hand a caller a sandbox the documentation does not describe."""
        run = boxdesk.dry_run(["true"], root=root)
        assert run.posture.vcpus == 1
        assert run.posture.mem_mib == 512
        assert run.posture.network == "none"
        assert run.posture.rootfs == "read-only"
        assert run.posture.results is True


class TestBoundaryRefusals:
    """``NonZeroU8`` and ``NonZeroU32`` are the core's types. Zero is refused HERE, with a
    sentence, rather than silently becoming the default on the way through."""

    def test_zero_vcpus_is_refused(self, boxdesk, root):
        with pytest.raises(ValueError, match="at least 1"):
            boxdesk.dry_run(["true"], root=root, vcpus=0)

    def test_zero_memory_is_refused(self, boxdesk, root):
        with pytest.raises(ValueError, match="at least 1"):
            boxdesk.dry_run(["true"], root=root, mem_mib=0)

    def test_an_unknown_posture_word_is_refused(self, boxdesk, root):
        # The dangerous version of this bug is silent: "writeable" falling through to read-only
        # gives a caller a sandbox they did not ask for and no sign of it.
        with pytest.raises(ValueError, match="read-only"):
            boxdesk.dry_run(["true"], root=root, rootfs="writeable")
        with pytest.raises(ValueError, match="none"):
            boxdesk.dry_run(["true"], root=root, net="host")

    def test_an_empty_command_is_refused(self, boxdesk, root):
        with pytest.raises(ValueError, match="empty"):
            boxdesk.dry_run([], root=root)

    def test_a_missing_guest_root_is_refused(self, boxdesk):
        with pytest.raises(RuntimeError):
            boxdesk.dry_run(["true"], root="/definitely/not/a/guest/root")


class TestDryRun:
    def test_it_has_no_end_and_no_directory(self, boxdesk, root):
        run = boxdesk.dry_run(["echo", "hi"], root=root)
        assert run.command == ["echo", "hi"]
        assert run.verb == "run"
        assert run.end_kind is None
        assert run.ended_ms is None
        assert run.dir is None
        assert run.ok is False

    def test_a_non_zero_exit_is_not_an_exception(self, boxdesk, root):
        """The heart of the contract. A guest command's own status says nothing about whether
        Boxdesk worked, so `ok` is the field to branch on and nothing raises for it.

        A dry run cannot produce a non-zero end, so this asserts the shape that decides it:
        `ok` is false for anything that is not a clean zero exit."""
        run = boxdesk.dry_run(["false"], root=root)
        assert run.ok is False
        assert run.end_code is None


class TestStore:
    def test_a_missing_run_names_what_was_asked_for(self, boxdesk):
        with pytest.raises(KeyError, match="no run"):
            boxdesk.show("1-definitely-not-a-run")

    def test_listing_runs_does_not_raise(self, boxdesk):
        # `runs` and `show` threw "not supported in FFI yet" for a while. That they answer at all
        # is the assertion; what is in the store depends on the machine.
        assert isinstance(boxdesk.runs(all=True), list)


def _flatten(value, depth: int = 0) -> str:
    """Every readable attribute of ``value``, recursively, as one string to search."""
    if depth > 4 or value is None or isinstance(value, (bool, int, float)):
        return repr(value)
    if isinstance(value, str):
        return value
    if isinstance(value, (list, tuple)):
        return " ".join(_flatten(v, depth + 1) for v in value)
    return " ".join(
        f"{name}={_flatten(getattr(value, name), depth + 1)}"
        for name in dir(value)
        if not name.startswith("_")
    )
