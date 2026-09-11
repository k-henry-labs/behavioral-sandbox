import os
from typing import Any, List, Optional, Sequence, Tuple

from .exceptions import TormoniError, TormoniNotFound
from .models import Run
from .options import Pairs, RunOptions, _entries

__all__ = ["Tormoni", "Sandbox"]

class Tormoni:
    def __init__(self, executable: Optional[str] = None) -> None:
        pass

    def run(self, command: Sequence[str], **kwargs: Any) -> Run:
        options = RunOptions(**kwargs)
        return self.run_with(command, options)

    def dry_run(self, command: Sequence[str], **kwargs: Any) -> Run:
        options = RunOptions(**kwargs)
        return self.run_with(command, options, dry_run=True)

    def run_with(self, command: Sequence[str], options: Optional[RunOptions] = None, *, dry_run: bool = False) -> Run:
        import tormoni._tormoni_core as core
        opts = options or RunOptions()
        
        def split_pair(p: str) -> Tuple[str, str]:
            parts = p.split("=", 1)
            return (parts[0], parts[1]) if len(parts) == 2 else (p, "")

        mounts = [split_pair(e) for e in _entries(opts.mounts, "mounts")] if opts.mounts else []
        shares = [split_pair(e) for e in _entries(opts.shares, "shares")] if opts.shares else []
        # The WHOLE `KEY=VALUE` entry. An earlier build sent only the name, so the guest was
        # handed a variable set to nothing; the record still keeps names alone, but it is the
        # core that makes that cut, not this file.
        env = list(_entries(opts.env, "env")) if opts.env else []

        try:
            return core.run_sandbox(
                opts.name,
                list(command),
                opts.root,
                opts.vcpus,
                opts.mem_mib,
                opts.workdir,
                mounts,
                shares,
                opts.net,
                opts.rootfs,
                env,
                opts.no_results,
                opts.keep,
                dry_run,
                opts.gpu,
                opts.sound,
            )
        except RuntimeError as e:
            if "No such file or directory" in str(e):
                raise TormoniNotFound(str(e))
            raise TormoniError(str(e), None)

    def show(self, id: str) -> Run:
        import tormoni._tormoni_core as core
        try:
            return core.show(id)
        except RuntimeError as e:
            if "No such file or directory" in str(e) or "not found" in str(e):
                raise TormoniNotFound(str(e))
            raise TormoniError(str(e), None)

    def runs(self, all: bool = False) -> List[Run]:
        import tormoni._tormoni_core as core
        try:
            return core.runs(all)
        except RuntimeError as e:
            raise TormoniError(str(e), None)

Sandbox = Tormoni
