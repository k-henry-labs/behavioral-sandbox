"""The options ``boxdesk run`` accepts, one field per flag.

Every field maps onto exactly one flag from the CLI's table.  This package
adds no options of its own and supplies no defaults of its own: a field left
``None`` (or ``False``) puts nothing on the command line, so the CLI's own
default stands.
"""

from dataclasses import dataclass
from typing import Iterable, Iterator, List, Mapping, Optional, Tuple, Union

__all__ = ["RunOptions", "Pairs"]

Pairs = Union[
    Mapping[str, str],
    Iterable[Tuple[str, str]],
    Iterable[str],
]
"""A repeatable ``KEY=VALUE`` flag, given as a mapping, as ``(key, value)``
pairs, or as literal ``"KEY=VALUE"`` strings.

Any iterable will do -- a list, a generator, ``some_dict.items()`` -- because
:class:`RunOptions` freezes it at construction, so a one-shot iterable is not
spent by the first :meth:`RunOptions.to_args` call."""


def _entries(value: Pairs, option: str) -> Iterator[str]:
    """Normalise a :data:`Pairs` argument into ``KEY=VALUE`` strings."""
    if isinstance(value, str):
        # A bare string is a sequence of characters; iterating it would build
        # one flag per letter. Almost certainly a forgotten list.
        raise TypeError(
            f"{option} must be a mapping or a sequence, not a bare string; "
            f"did you mean [{value!r}]?"
        )
    if isinstance(value, Mapping):
        for key, val in value.items():
            yield f"{key}={val}"
        return
    for entry in value:
        if isinstance(entry, str):
            if "=" not in entry:
                # A bare ("guest", "host") pair lands here as two separate
                # strings and would otherwise become two malformed flags.
                raise ValueError(
                    f"string entries for {option} must be KEY=VALUE, got "
                    f"{entry!r}; did you pass a ('key', 'value') pair instead "
                    f"of a sequence of pairs?"
                )
            yield entry
            continue
        try:
            key, val = entry
        except (TypeError, ValueError):
            raise TypeError(
                f"entries for {option} must be KEY=VALUE strings or "
                f"(key, value) pairs, got {entry!r}"
            ) from None
        yield f"{key}={val}"


def _canonical(value: Optional[Pairs], option: str) -> Optional[Tuple[str, ...]]:
    """Validate a :data:`Pairs` argument and freeze it into ``KEY=VALUE`` form.

    Doing this at construction means a malformed option is reported where the
    caller wrote it, and that a one-shot iterable is not spent by the first
    :meth:`RunOptions.to_args` call.
    """
    if value is None:
        return None
    return tuple(_entries(value, option))


@dataclass(frozen=True)
class RunOptions:
    """Options for :meth:`boxdesk.Boxdesk.run` and
    :meth:`boxdesk.Boxdesk.dry_run`."""

    root: Optional[str] = None
    """``--root DIR``: the guest root tree."""
    vcpus: Optional[int] = None
    """``--vcpus N``."""
    mem_mib: Optional[int] = None
    """``--mem MIB``: guest RAM."""
    workdir: Optional[str] = None
    """``--workdir DIR``: the guest working directory."""
    mounts: Optional[Pairs] = None
    """``--mount GUESTDIR=HOSTDIR``: host directories, read-write, at guest
    paths."""
    shares: Optional[Pairs] = None
    """``--share TAG=HOSTPATH``: extra virtiofs devices the guest mounts by
    tag."""
    net: Optional[str] = None
    """``--net none|tsi``. ``none`` means no network at all."""
    rootfs: Optional[str] = None
    """``--rootfs read-only|writable``."""
    env: Optional[Pairs] = None
    """``--env KEY=VALUE``. The guest gets the whole entry; the record keeps
    only the name."""
    name: Optional[str] = None
    """``--name NAME``: names the sandbox."""
    no_results: bool = False
    """``--no-results``: drops the default ``/results`` mount."""
    keep: bool = False
    """``--keep``: files the run's record instead of sweeping it.

    A run is ephemeral by default: its output comes back in the result and the
    directory it worked in goes with it, so :attr:`Run.files` is empty and
    :attr:`Run.dir` is ``None``.  Keep it when you mean to read what the guest
    wrote to ``/results``, or to reach the run again with
    :meth:`boxdesk.Boxdesk.show`."""
    gpu: bool = False
    """``--gpu``. Refused by a host whose libkrun was built without it."""
    sound: bool = False
    """``--sound``. Refused by a host whose libkrun was built without it."""
    display: Optional[str] = None
    """``--display WxH[@HZ]``. Does not currently boot on macOS."""

    def __post_init__(self) -> None:
        # Frozen, so the canonical form goes in the same way dataclasses.replace
        # would put it: through object.__setattr__.
        for option in ("mounts", "shares", "env"):
            value: Optional[Pairs] = getattr(self, option)
            object.__setattr__(self, option, _canonical(value, option))

    def to_args(self) -> List[str]:
        """Render these options as argv, in the order of the CLI's table."""
        args: List[str] = []
        if self.root is not None:
            args += ["--root", self.root]
        if self.vcpus is not None:
            args += ["--vcpus", str(self.vcpus)]
        if self.mem_mib is not None:
            args += ["--mem", str(self.mem_mib)]
        if self.workdir is not None:
            args += ["--workdir", self.workdir]
        if self.mounts is not None:
            for entry in _entries(self.mounts, "mounts"):
                args += ["--mount", entry]
        if self.shares is not None:
            for entry in _entries(self.shares, "shares"):
                args += ["--share", entry]
        if self.net is not None:
            args += ["--net", self.net]
        if self.rootfs is not None:
            args += ["--rootfs", self.rootfs]
        if self.env is not None:
            for entry in _entries(self.env, "env"):
                args += ["--env", entry]
        if self.name is not None:
            args += ["--name", self.name]
        if self.no_results:
            args.append("--no-results")
        if self.keep:
            args.append("--keep")
        if self.gpu:
            args.append("--gpu")
        if self.sound:
            args.append("--sound")
        if self.display is not None:
            args += ["--display", self.display]
        return args
