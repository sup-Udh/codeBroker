$ErrorActionPreference = "Stop"

$Repo = "sup-Udh/codeBroker"
$BinDir = "$env:USERPROFILE\.codebroker\bin"
$ExeName = "codebroker.exe"
$AssetName = "codebroker-windows-x86_64.zip"

Write-Host "Installing CodeBroker..." -ForegroundColor Cyan

# Fetch latest release data from GitHub API
$ApiUrl = "https://api.github.com/repos/$Repo/releases/latest"
Write-Host "Querying latest release from $ApiUrl"
try {
    $ReleaseData = Invoke-RestMethod -Uri $ApiUrl -ErrorAction Stop
} catch {
    Write-Host "Failed to fetch release data. Please check your internet connection and GitHub API limits." -ForegroundColor Red
    exit 1
}

$DownloadUrl = $null
foreach ($Asset in $ReleaseData.assets) {
    if ($Asset.name -eq $AssetName) {
        $DownloadUrl = $Asset.browser_download_url
        break
    }
}

if ($null -eq $DownloadUrl) {
    Write-Host "Could not find a release for Windows x86_64." -ForegroundColor Red
    Write-Host "Please check https://github.com/$Repo/releases" -ForegroundColor Red
    exit 1
}

Write-Host "Downloading latest release: $DownloadUrl"

# Prepare directories
if (-not (Test-Path -Path $BinDir)) {
    New-Item -ItemType Directory -Path $BinDir | Out-Null
}

$TempZip = Join-Path -Path $env:TEMP -ChildPath $AssetName
$TempExtracted = Join-Path -Path $env:TEMP -ChildPath "codebroker_extracted"

if (Test-Path -Path $TempExtracted) {
    Remove-Item -Path $TempExtracted -Recurse -Force | Out-Null
}
New-Item -ItemType Directory -Path $TempExtracted | Out-Null

# Download
Invoke-WebRequest -Uri $DownloadUrl -OutFile $TempZip

# Extract
Write-Host "Extracting..."
Expand-Archive -Path $TempZip -DestinationPath $TempExtracted -Force

# Move the executable
$SourceExe = Join-Path -Path $TempExtracted -ChildPath $ExeName
$DestExe = Join-Path -Path $BinDir -ChildPath $ExeName
Move-Item -Path $SourceExe -Destination $DestExe -Force

# Cleanup temp files
Remove-Item -Path $TempZip -Force | Out-Null
Remove-Item -Path $TempExtracted -Recurse -Force | Out-Null

Write-Host "Successfully installed $ExeName to $BinDir" -ForegroundColor Green

# Add to PATH if not already there
$UserPath = [Environment]::GetEnvironmentVariable("PATH", "User")
if ($UserPath -notlike "*$BinDir*") {
    Write-Host "Adding $BinDir to your user PATH..."
    $NewPath = "$UserPath;$BinDir"
    [Environment]::SetEnvironmentVariable("PATH", $NewPath, "User")
    
    # Also update current session PATH
    $env:PATH = "$env:PATH;$BinDir"
    
    Write-Host "PATH updated successfully. You might need to restart your terminal to use 'codebroker'." -ForegroundColor Yellow
}

Write-Host "`nInstallation complete! Run 'codebroker --help' to get started." -ForegroundColor Cyan
