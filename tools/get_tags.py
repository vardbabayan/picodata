#!/usr/bin/env python3

import argparse
import subprocess
import pathlib
import os
import time


SUBMODULES_TO_FETCH_TAGS = os.environ.get('SUBMODULES_TO_FETCH_TAGS', ['.',
                                          'tarantool-sys', 'tarantool-sys/third_party/luajit'])
if type(SUBMODULES_TO_FETCH_TAGS) is str:
    SUBMODULES_TO_FETCH_TAGS = SUBMODULES_TO_FETCH_TAGS.split(",")
GET_SOURCES_ATTEMPTS = int(os.environ.get('GET_SOURCES_ATTEMPTS', 3))
PROJECT_DIR = pathlib.Path(__file__).parent.parent


def run_shell(path, shell=True, executable='/bin/bash', text=True):
    retry = GET_SOURCES_ATTEMPTS
    limit = 100
    timeout = 3
    while retry > 0:
        try:
            while True:
                result = ""
                print(path)
                proc = subprocess.run("git describe",
                                      shell=shell, executable=executable, text=text,
                                      cwd="{}/{}".format(PROJECT_DIR, path))
                result = proc.stdout
                code = proc.returncode
                if not code:
                    return

                print("fetching tag for", path)
                proc = subprocess.run(
                    "git fetch --deepen 50",
                    shell=shell,
                    executable=executable,
                    text=text,
                    cwd="{}/{}".format(PROJECT_DIR, path),
                )
                print("stdout={}, stderr={}, code={}".format(
                    proc.stdout, proc.stderr, proc.returncode))
                limit -= 1
                if limit < 0:
                    print("can't fetch tags")
                    return 2
        except Exception as e:
            print("can't run: " + str(e))
            retry -= 1
            time.sleep(timeout)
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(prog="GetGitTags", description="Get project tags")
    parser.add_argument("dirs", nargs="*", default=SUBMODULES_TO_FETCH_TAGS, type=str)
    args = parser.parse_args()

    for path in args.dirs:
        t0 = time.time()
        run_shell(path)
        print(path, "elapsed", time.time() - t0)
