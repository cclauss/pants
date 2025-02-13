# Copyright 2019 Pants project contributors (see CONTRIBUTORS.md).
# Licensed under the Apache License, Version 2.0 (see LICENSE).

from __future__ import annotations

from typing import Tuple, cast

from pants.backend.python.subsystems.python_tool_base import PythonToolBase
from pants.backend.python.target_types import ConsoleScript
from pants.engine.addresses import UnparsedAddressInputs
from pants.option.custom_types import file_option, shell_str, target_option


class MyPy(PythonToolBase):
    options_scope = "mypy"
    help = "The MyPy Python type checker (http://mypy-lang.org/)."

    default_version = "mypy==0.800"
    default_main = ConsoleScript("mypy")
    # See `mypy/rules.py`. We only use these default constraints in some situations. Technically,
    # MyPy only requires 3.5+, but some popular plugins like `django-stubs` require 3.6+. Because
    # 3.5 is EOL, and users can tweak this back, this seems like a more sensible default.
    register_interpreter_constraints = True
    default_interpreter_constraints = ["CPython>=3.6"]

    @classmethod
    def register_options(cls, register):
        super().register_options(register)
        register(
            "--skip",
            type=bool,
            default=False,
            help=f"Don't use MyPy when running `{register.bootstrap.pants_bin_name} typecheck`.",
        )
        register(
            "--args",
            type=list,
            member_type=shell_str,
            help=(
                "Arguments to pass directly to mypy, e.g. "
                f'`--{cls.options_scope}-args="--python-version 3.7 --disallow-any-expr"`'
            ),
        )
        register(
            "--config",
            type=file_option,
            advanced=True,
            help="Path to `mypy.ini` or alternative MyPy config file",
        )
        register(
            "--source-plugins",
            type=list,
            member_type=target_option,
            advanced=True,
            help=(
                "An optional list of `python_library` target addresses to load first-party "
                "plugins.\n\nYou must also set `plugins = path.to.module` in your `mypy.ini`, and "
                "set the `[mypy].config` option in your `pants.toml`.\n\nTo instead load "
                "third-party plugins, set the option `[mypy].extra_requirements` and set the "
                "`plugins` option in `mypy.ini`."
            ),
        )

    @property
    def skip(self) -> bool:
        return cast(bool, self.options.skip)

    @property
    def args(self) -> Tuple[str, ...]:
        return tuple(self.options.args)

    @property
    def config(self) -> str | None:
        return cast("str | None", self.options.config)

    @property
    def source_plugins(self) -> UnparsedAddressInputs:
        return UnparsedAddressInputs(self.options.source_plugins, owning_address=None)
