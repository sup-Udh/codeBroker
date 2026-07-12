$ErrorActionPreference = "Stop"

$Repo = "sup-Udh/codeBroker"
$BinDir = "$env:USERPROFILE\.codebroker\bin"
$ExeName = "codebroker.exe"
$McpExeName = "codebroker-mcp.exe"
$AssetName = "codebroker-windows-x86_64.zip"

Write-Host "Installing CodeBroker..." -ForegroundColor Cyan

$R2Url = "https://www.codebroker.space"
$DownloadUrl = "$R2Url/$AssetName"

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

# Move the executables
$SourceExe = Join-Path -Path $TempExtracted -ChildPath $ExeName
$DestExe = Join-Path -Path $BinDir -ChildPath $ExeName
Move-Item -Path $SourceExe -Destination $DestExe -Force

$SourceMcpExe = Join-Path -Path $TempExtracted -ChildPath $McpExeName
$DestMcpExe = Join-Path -Path $BinDir -ChildPath $McpExeName
Move-Item -Path $SourceMcpExe -Destination $DestMcpExe -Force

# Cleanup temp files
Remove-Item -Path $TempZip -Force | Out-Null
Remove-Item -Path $TempExtracted -Recurse -Force | Out-Null

Write-Host "Successfully installed $ExeName and $McpExeName to $BinDir" -ForegroundColor Green

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
