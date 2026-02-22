#!/usr/bin/env python3
# SPDX-License-Identifier: LGPL-2.1-or-later
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


def find_wit_dir() -> str:
    """Find the WIT directory for simple-actor interface."""
    # Check common locations
    candidates = [
        # Relative to this file (in SDK)
        Path(__file__).parent.parent.parent.parent / "wit" / "plexspaces-simple-actor",
        # Relative to workspace root
        Path.cwd() / "wit" / "plexspaces-simple-actor",
        # Environment variable
        Path(os.environ.get("PLEXSPACES_WIT_DIR", "")) / "plexspaces-simple-actor",
    ]
    
    for candidate in candidates:
        if candidate.exists() and (candidate / "world.wit").exists():
            return str(candidate)
    
    raise FileNotFoundError(
        "Could not find WIT directory. Set PLEXSPACES_WIT_DIR or use --wit-dir"
    )


def generate_wrapper(actor_file: Path, work_dir: Path) -> Path:
    """
    Generate the WIT interface wrapper for an actor file.

    Supports multiple @actor classes in one file (Erlang-style ApplicationSpec).
    Returns path to the generated wrapper file.
    """
    import importlib.util

    # Copy actor file to work directory
    actor_name = actor_file.stem
    shutil.copy(actor_file, work_dir / actor_file.name)

    # Load the module to find @actor classes
    spec = importlib.util.spec_from_file_location(actor_name, actor_file)
    module = importlib.util.module_from_spec(spec)

    # Add plexspaces to path
    sdk_path = Path(__file__).parent.parent.parent
    sys.path.insert(0, str(sdk_path))

    spec.loader.exec_module(module)

    # Find ALL @actor classes
    actor_classes = []
    for name in dir(module):
        obj = getattr(module, name)
        if isinstance(obj, type) and hasattr(obj, '_plexspaces_is_actor'):
            actor_classes.append(obj)

    if not actor_classes:
        # No @actor decorator - assume it's already a WIT-compatible module
        # Just use it as-is (backward compatible with old examples)
        return actor_file

    # Generate wrapper using SDK (supports single or multi-actor)
    from plexspaces.runtime import generate_wrapper as gen_wrapper

    wrapper_code = gen_wrapper(actor_classes, actor_name)
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
        import importlib.util
        sdk_path = Path(__file__).parent.parent.parent
        sys.path.insert(0, str(sdk_path))
        
        spec = importlib.util.spec_from_file_location(actor_name, actor_path)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        
        # Find @actor class(es)
        has_sdk_actor = any(
            isinstance(getattr(module, name), type) and hasattr(getattr(module, name), '_plexspaces_is_actor')
            for name in dir(module)
        )
        
        if has_sdk_actor:
            # Generate wrapper for SDK-style actor
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
            
            cmd_bindings = [
                "componentize-py", "-d", wit_dir, "-w", world, "bindings", "."
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
            
            cmd_build = [
                "componentize-py", "-d", wit_dir, "-w", world,
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
