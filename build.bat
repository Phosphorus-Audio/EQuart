@echo off
setlocal
cd /d "%~dp0"

echo === PPA EQuart build ===

rem ---------- parse args ----------
rem usage: build.bat [release^|debug] [install] [trace]
rem   release (default): cargo xtask bundle --release
rem   debug            : cargo xtask bundle (debug)
rem   install          : additionally copy bundle to system plugin dirs
rem   trace            : also pass --features dpi-trace, which writes a
rem                      per-second frame report (fps, ui/render/present
rem                      times, GPU adapter) to LOCALAPPDATA/f0lock/diagnostics

set "PROFILE=--release"
set "OUTDIR=target\bundled"
set "DO_INSTALL=0"
set "FEATURES="

:parse
if "%~1"=="" goto run
if /i "%~1"=="debug" (
    set "PROFILE="
    set "OUTDIR=target\debug\bundled"
    shift
    goto parse
)
if /i "%~1"=="release" (
    shift
    goto parse
)
if /i "%~1"=="install" (
    set "DO_INSTALL=1"
    shift
    goto parse
)
if /i "%~1"=="trace" (
    set "FEATURES=--features dpi-trace"
    shift
    goto parse
)
echo unknown argument: %~1
echo usage: build.bat [release^|debug] [install] [trace]
exit /b 1

:run
where cargo >nul 2>nul
if errorlevel 1 (
    echo [error] cargo not found in PATH. Install Rust first: https://rustup.rs
    exit /b 1
)

echo.
echo [1/2] cargo xtask bundle equart %PROFILE% %FEATURES%
cargo xtask bundle equart %PROFILE% %FEATURES%
if errorlevel 1 (
    echo.
    echo [error] build failed.
    exit /b 1
)

echo.
echo [2/2] bundle output:
if exist "%OUTDIR%\equart.clap"      echo   %OUTDIR%\equart.clap
if exist "%OUTDIR%\equart.vst3"      echo   %OUTDIR%\equart.vst3\
if not exist "%OUTDIR%\equart.clap" if not exist "%OUTDIR%\equart.vst3" (
    echo   [warn] no bundle found under %OUTDIR%
)

if "%DO_INSTALL%"=="1" call :install

echo.
echo === done ===
pause
exit /b 0

:install
if not exist "%OUTDIR%\equart.clap" if not exist "%OUTDIR%\equart.vst3" (
    echo [error] nothing to install.
    exit /b 1
)
set "CLAP_DIR=%COMMONPROGRAMFILES%\CLAP"
set "VST3_DIR=%COMMONPROGRAMFILES%\VST3"
echo installing to:
echo   %CLAP_DIR%
echo   %VST3_DIR%
if not exist "%CLAP_DIR%" mkdir "%CLAP_DIR%"
if not exist "%VST3_DIR%" mkdir "%VST3_DIR%"
if exist "%OUTDIR%\equart.clap" copy /y "%OUTDIR%\equart.clap" "%CLAP_DIR%\" >nul
if exist "%OUTDIR%\equart.vst3" robocopy "%OUTDIR%\equart.vst3" "%VST3_DIR%\equart.vst3" /e /njh /njs >nul
if errorlevel 8 (
    echo [error] install failed - run this script as Administrator.
    exit /b 1
)
echo installed.
exit /b 0
