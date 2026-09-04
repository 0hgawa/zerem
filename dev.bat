@echo off
rem Build and run, teeing everything to dev.log. Mirrors Clipo's and
rem vayou-slint's dev.bat, with modes this project needs: the Phase 0 table
rem spike is still owed a by-hand scroll test on both renderers, and that
rem comparison is the reason for the `table` / `table gpu` modes below.
rem
rem   dev.bat              the app, debug          (default)
rem   dev.bat release      the app, release        (the only build worth measuring)
rem   dev.bat table        table spike, software renderer
rem   dev.bat table gpu    table spike, femtovg    (the other half of the comparison)

setlocal
cd /d "%~dp0"
set LOG=dev.log

rem Verbose by default: the point of this script is watching a running app, and
rem at the default (warn) the log shows nothing. Values: trace, debug, info,
rem warn, error. Override with:  set ZEREM_LOG=info  &&  dev.bat
if not defined ZEREM_LOG set ZEREM_LOG=debug

rem A panic on the engine thread otherwise surfaces as a bare "thread panicked"
rem with no frames, which is useless when diagnosing.
set RUST_BACKTRACE=1

rem rustup installs here but does not always end up on PATH for a fresh shell.
where cargo >nul 2>&1
if errorlevel 1 set PATH=%USERPROFILE%\.cargo\bin;%PATH%
where cargo >nul 2>&1
if errorlevel 1 (
  echo.
  echo   cargo not found. Install Rust from https://rustup.rs and try again.
  echo.
  pause
  exit /b 1
)

if /i "%~1"=="table" goto :table
if /i "%~1"=="release" goto :release
if "%~1"=="" goto :app
echo Unknown mode "%~1". Run dev.bat with no arguments, or: release ^| table ^| table gpu
exit /b 1

rem ---------------------------------------------------------------- the app --

:app
set PROFILE=
set WHAT=debug
goto :run_app

:release
set PROFILE=--release
set WHAT=release
goto :run_app

:run_app
echo === Zerem ^(Slint^) dev - %WHAT% ===
echo Log file:   %CD%\%LOG%
echo ZEREM_LOG:  %ZEREM_LOG%
echo.
echo The engine is still the synthetic one - 2000 fake torrents, ticking once
echo a second. What is worth trying:
echo.
echo   click a header      sorts; click again inverts
echo   drag a header edge  resizes that column
echo   click / Ctrl / Shift  select one, toggle, range
echo   Space               pause or start the selection
echo   Delete              remove the selection
echo   T                   light / dark
echo.
echo Each tick prints its cost to the log: "changed" is how many rows the diff
echo dirtied, and "reset=true" on anything but the first tick would mean the
echo scroll position was thrown away.
if "%WHAT%"=="debug" echo.
if "%WHAT%"=="debug" echo Numbers from a debug build mean nothing - use "dev.bat release" to measure.
echo.

powershell -NoProfile -ExecutionPolicy Bypass -Command "& { cargo run %PROFILE% 2>&1 | ForEach-Object { $_.ToString() } | Tee-Object -FilePath '%LOG%'; exit $LASTEXITCODE }"
set ERR=%ERRORLEVEL%
goto :done

rem -------------------------------------------------------- the table spike --

:table
rem The one Phase 0 criterion still owed a human: partial repainting is what
rem makes the software renderer cheap, and it cannot help when the whole
rem viewport moves. Run both, scroll each end to end, and compare.
rem No parentheses in these values: inside an if-block cmd would try to parse
rem them as block delimiters.
set SLINT_BACKEND=winit-software
set RENDERER=software
if /i "%~2"=="gpu" set SLINT_BACKEND=winit-femtovg
if /i "%~2"=="gpu" set RENDERER=femtovg, the GPU path

echo === Zerem table spike - %RENDERER% ===
echo.
echo Scroll the list from top to bottom and back, repeatedly. What you are
echo looking for is tearing or a stutter - anything that does not feel like a
echo native list.
echo.
echo Measured for reference: software costs 17 MB of private memory against
echo femtovg's 120 MB, and a fifth of the CPU under repaint load. If software
echo holds up while scrolling, it stays the default and the app is 7x lighter.
echo.
echo   S       stress - repaint flat out, to find the fps ceiling
echo   C       churn - every row changes each tick, the worst case
echo   T       light / dark
echo   1 2 3   200 / 2000 / 10000 rows
echo.
if /i "%~2"=="gpu" (
  echo The fps counter only works here: the software renderer has no rendering
  echo notifier, and prints 0.
) else (
  echo fps will read 0 - the software renderer offers no rendering notifier.
  echo Run "dev.bat table gpu" for the counter.
)
echo.

cd spikes\table
powershell -NoProfile -ExecutionPolicy Bypass -Command "& { cargo run --release 2>&1 | ForEach-Object { $_.ToString() } | Tee-Object -FilePath '..\..\%LOG%'; exit $LASTEXITCODE }"
rem Captured before the `cd` below, which would overwrite it with its own 0.
set ERR=%ERRORLEVEL%
cd /d "%~dp0"
goto :done

rem ---------------------------------------------------------------------------

:done
echo.
echo ===================================
echo Exit code: %ERR%
echo Full log:  %CD%\%LOG%
echo ===================================
pause
endlocal
