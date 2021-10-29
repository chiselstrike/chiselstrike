#!/usr/bin/env python3
#
# SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

project = 'ChiselStrike'
copyright = '2021, ChiselStrike Inc.'
author = 'ChiselStrike Inc.'

extensions = ['myst_parser']

templates_path = ['_templates']

exclude_patterns = ['_build', 'Thumbs.db', '.DS_Store']

html_theme = 'alabaster'

html_static_path = ['_static']

source_suffix = ['.rst', '.md']
