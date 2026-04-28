#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2025 PlexSpaces Contributors
#
# PlexSpaces Build Tool
#
# Builds Python actors into WASM components with minimal boilerplate.
# Replaces the manual build.sh scripts.

"""
PlexSpaces Python build tool.

Usage:
    plexspaces-py build myactor.py -o myactor.wasm
    plexspaces-py build myactor.py --wit-dir /path/to/wit

This tool:
1. Reads the Python file with @actor decorated class
2. Generates a WIT interface wrapper
3. Runs componentize-py to create bindings
4. Runs componentize-py to create WASM component
"""

import argparse
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path


def componentize_command() -> list[str]:
    """Return a componentize-py invocation bound to the current interpreter."""
    return [
        sys.executable,
        "-c",
        (
            "import sys; "
            "from componentize_py import script; "
            "sys.argv[0] = 'componentize-py'; "
            "sys.exit(script())"
        ),
    ]


def find_wit_dir() -> str:
    """Find the WIT directory for the actor-world interface."""
    # Check common locations
    candidates = [
        # Relative to this file (in SDK)
        Path(__file__).parent.parent.parent.parent / "wit" / "plexspaces-actor",
        # Relative to workspace root
        Path.cwd() / "wit" / "plexspaces-actor",
        # Environment variable
        Path(os.environ.get("PLEXSPACES_WIT_DIR", "")) / "plexspaces-actor",
    ]
    
    for candidate in candidates:
        if candidate.exists() and (candidate / "world.wit").exists():
            return str(candidate)
    
    raise FileNotFoundError(
        "Could not find WIT directory. Set PLEXSPACES_WIT_DIR or use --wit-dir"
    )


def _load_actor_module(actor_name: str, actor_path: Path):
    """Load an actor module, supporting both single-file and multi-file package layouts.

    When sibling .py files exist in the same directory the actor file is treated as
    part of a package.  The parent directory is added to sys.path and the module is
    imported via importlib with an explicit package name so that relative imports
    (``from .helpers import …``) resolve correctly.
    """
    import importlib
    import importlib.util
    import types

    actor_dir = actor_path.parent

    # Detect multi-file package: any sibling .py file (other than the actor itself)
    siblings = [p for p in actor_dir.glob("*.py") if p != actor_path]
    if siblings:
        # Install the parent directory as the package root so relative imports work.
        if str(actor_dir) not in sys.path:
            sys.path.insert(0, str(actor_dir))

        # Register the package (the actor's stem is used as the package name).
        pkg_name = actor_path.parent.name  # e.g. "miniclaw"
        module_name = f"{pkg_name}.{actor_name}"

        # Bootstrap the package object if it isn't already importable
        if pkg_name not in sys.modules:
            pkg_spec = importlib.util.spec_from_file_location(
                pkg_name,
                actor_dir / "__init__.py" if (actor_dir / "__init__.py").exists() else None,
                submodule_search_locations=[str(actor_dir)],
            )
            if pkg_spec is not None and pkg_spec.loader is not None:
                pkg = importlib.util.module_from_spec(pkg_spec)
                sys.modules[pkg_name] = pkg
                pkg_spec.loader.exec_module(pkg)
            else:
                # No __init__.py — create a namespace package
                pkg = types.ModuleType(pkg_name)
                pkg.__path__ = [str(actor_dir)]  # type: ignore[attr-defined]
                pkg.__package__ = pkg_name
                sys.modules[pkg_name] = pkg

        spec = importlib.util.spec_from_file_location(
            module_name,
            actor_path,
            submodule_search_locations=[],
        )
        module = importlib.util.module_from_spec(spec)
        module.__package__ = pkg_name
        sys.modules[module_name] = module
        spec.loader.exec_module(module)
    else:
        # Single-file actor — original simple path
        spec = importlib.util.spec_from_file_location(actor_name, actor_path)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)

    return module


def _copy_py_flat(src: Path, dest: Path) -> None:
    """Copy a Python file to dest, rewriting relative imports to absolute ones.

    componentize-py runs in a flat temp directory with no package context, so
    ``from .module import X`` must become ``from module import X``.
    """
    import re
    text = src.read_text(encoding="utf-8")
    # "from .foo import ..." → "from foo import ..."
    text = re.sub(r'^from \.([\w]+)', r'from \1', text, flags=re.MULTILINE)
    # "from . import foo" → "import foo"
    text = re.sub(r'^from \. import ', r'import ', text, flags=re.MULTILINE)
    dest.write_text(text, encoding="utf-8")


def generate_wrapper(actor_file: Path, work_dir: Path) -> Path:
    """
    Generate the WIT interface wrapper for an actor file.

    Supports multiple @actor classes in one file (Erlang-style ApplicationSpec).
    Returns path to the generated wrapper file.
    """
    # Copy actor file and all sibling .py files to work directory so componentize-py
    # can find them at build time.  Relative imports are rewritten to absolute so they
    # work in the flat temp directory where componentize-py runs.
    actor_name = actor_file.stem
    actor_dir = actor_file.parent
    _copy_py_flat(actor_file, work_dir / actor_file.name)
    for sibling in actor_dir.glob("*.py"):
        if sibling != actor_file:
            _copy_py_flat(sibling, work_dir / sibling.name)

    # Load the module to find @actor classes
    # Add plexspaces to path
    sdk_path = Path(__file__).parent.parent.parent
    sys.path.insert(0, str(sdk_path))

    module = _load_actor_module(actor_name, actor_file)

    # Find ALL @actor classes
    actor_classes = []
    for name in dir(module):
        obj = getattr(module, name)
        if isinstance(obj, type) and hasattr(obj, '_plexspaces_is_actor'):
            actor_classes.append(obj)

    actor_roles = getattr(module, "ACTOR_ROLES", None)

    if not actor_classes:
        # No @actor decorator - assume it's already a WIT-compatible module
        # Just use it as-is (backward compatible with old examples)
        return actor_file

    # Generate wrapper using SDK (supports single or multi-actor)
    from plexspaces.runtime import generate_wrapper as gen_wrapper

    wrapper_code = gen_wrapper(actor_classes, actor_name, actor_roles=actor_roles)
    wrapper_path = work_dir / f"{actor_name}_actor.py"

    with open(wrapper_path, 'w') as f:
        f.write(wrapper_code)

    return wrapper_path


def build_wasm(
    actor_file: str,
    output: str = None,
    wit_dir: str = None,
    world: str = "actor-world",
    clean: bool = True,
    verbose: bool = False,
) -> str:
    """
    Build a Python actor into a WASM component.
    
    Args:
        actor_file: Path to the Python file with @actor class
        output: Output WASM file path (default: {name}_actor.wasm)
        wit_dir: Path to WIT directory (default: auto-detect)
        world: WIT world name (default: actor-world)
        clean: Clean up generated files after build
        verbose: Print verbose output
    
    Returns:
        Path to the generated WASM file
    """
    actor_path = Path(actor_file).resolve()
    
    if not actor_path.exists():
        raise FileNotFoundError(f"Actor file not found: {actor_file}")
    
    # Determine output path (must be absolute since we cd to temp dir)
    actor_name = actor_path.stem
    if output is None:
        # Remove _actor suffix if present, then add it back
        base_name = actor_name.replace("_actor", "")
        output = str(actor_path.parent / f"{base_name}_actor.wasm")
    else:
        # Resolve relative paths against current directory before cd
        output = str(Path(output).resolve())
    
    # Find WIT directory
    if wit_dir is None:
        wit_dir = find_wit_dir()
    
    if verbose:
        print(f"Building {actor_path.name}")
        print(f"  WIT dir: {wit_dir}")
        print(f"  Output: {output}")
    
    # Create working directory
    with tempfile.TemporaryDirectory() as temp_dir:
        work_dir = Path(temp_dir)
        
        # Check if this is an SDK-style actor or legacy WIT-compatible
        sdk_path = Path(__file__).parent.parent.parent
        sys.path.insert(0, str(sdk_path))

        module = _load_actor_module(actor_name, actor_path)

        # Find @actor class(es)
        has_sdk_actor = any(
            isinstance(getattr(module, name), type) and hasattr(getattr(module, name), '_plexspaces_is_actor')
            for name in dir(module)
        )

        if has_sdk_actor:
            # Generate wrapper for SDK-style actor (also copies sibling files)
            wrapper_path = generate_wrapper(actor_path, work_dir)
            build_module = wrapper_path.stem
        else:
            # Legacy WIT-compatible actor - copy to work dir
            shutil.copy(actor_path, work_dir / actor_path.name)
            build_module = actor_name
        
        # Copy plexspaces SDK to work directory
        sdk_src = Path(__file__).parent.parent / "plexspaces"
        sdk_dest = work_dir / "plexspaces"
        if sdk_src.exists():
            shutil.copytree(sdk_src, sdk_dest)
        
        # Change to work directory for componentize-py
        old_cwd = os.getcwd()
        os.chdir(work_dir)
        
        try:
            # Clean any existing bindings
            for item in ["wit_world", "componentize_py_types.py", 
                        "componentize_py_runtime.pyi", "poll_loop.py",
                        "componentize_py_async_support"]:
                if (work_dir / item).exists():
                    if (work_dir / item).is_dir():
                        shutil.rmtree(work_dir / item)
                    else:
                        (work_dir / item).unlink()
            
            # Generate bindings
            if verbose:
                print("  Generating WIT bindings...")
            
            cmd_bindings = componentize_command() + [
                "-d", wit_dir, "-w", world, "bindings", "."
            ]
            result = subprocess.run(
                cmd_bindings,
                capture_output=not verbose,
                text=True
            )
            if result.returncode != 0:
                print(f"Error generating bindings: {result.stderr}")
                raise RuntimeError("componentize-py bindings failed")
            
            # Build WASM component
            if verbose:
                print("  Building WASM component...")
            
            cmd_build = componentize_command() + [
                "-d", wit_dir, "-w", world,
                "componentize", "-o", str(output), build_module
            ]
            result = subprocess.run(
                cmd_build,
                capture_output=not verbose,
                text=True
            )
            if result.returncode != 0:
                print(f"Error building component: {result.stderr}")
                raise RuntimeError("componentize-py componentize failed")
            
        finally:
            os.chdir(old_cwd)
    
    # Verify output
    if not Path(output).exists():
        raise RuntimeError(f"Build failed - output not created: {output}")
    
    output_size = Path(output).stat().st_size
    if verbose:
        print(f"  Built: {output} ({output_size:,} bytes)")
    
    return output


def main():
    """CLI entry point."""
    parser = argparse.ArgumentParser(
        prog="plexspaces-py",
        description="PlexSpaces Python SDK build tool"
    )
    
    subparsers = parser.add_subparsers(dest="command", help="Commands")
    
    # Build command
    build_parser = subparsers.add_parser("build", help="Build a Python actor to WASM")
    build_parser.add_argument("actor_file", help="Python file with @actor class")
    build_parser.add_argument("-o", "--output", help="Output WASM file")
    build_parser.add_argument("--wit-dir", help="WIT directory path")
    build_parser.add_argument("--world", default="actor-world", help="WIT world name")
    build_parser.add_argument("-v", "--verbose", action="store_true", help="Verbose output")
    
    # Version command
    parser.add_argument("--version", action="version", version="plexspaces-py 0.1.0")
    
    args = parser.parse_args()
    
    if args.command == "build":
        try:
            output = build_wasm(
                args.actor_file,
                output=args.output,
                wit_dir=args.wit_dir,
                world=args.world,
                verbose=args.verbose,
            )
            print(f"✅ Built: {output}")
        except Exception as e:
            print(f"❌ Build failed: {e}")
            sys.exit(1)
    else:
        parser.print_help()
        sys.exit(1)


if __name__ == "__main__":
    main()
