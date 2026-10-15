param(
 [string]$Plugins='storage,web', [switch]$Choose, [switch]$Open, [switch]$NoService,
 [string]$Name=$env:COMPUTERNAME, [string]$Listen='127.0.0.1', [int]$Port=7700,
 [string]$PublicUrl='', [string]$JoinUrl='', [string]$JoinCode='', [switch]$TrustHub
)
$ErrorActionPreference='Stop'
if ($env:PROCESSOR_ARCHITECTURE -ne 'AMD64') { throw 'Windows x64 is required.' }
$version=if($env:DELTA_VERSION){$env:DELTA_VERSION}else{'v0.3.0'}
$homeDir=if($env:DELTA_HOME){$env:DELTA_HOME}else{Join-Path $env:LOCALAPPDATA 'Delta'}
$env:DELTA_HOME=$homeDir
if($Choose){$choice=Read-Host 'Install [1] node + web, [2] node + web + typing, [3] bare node';$Plugins=switch($choice){'2'{'storage,web,typing'}'3'{'none'}default{'storage,web'}};$Choose=$false}
if($env:DELTA_FORCE_DOWNLOAD -ne '1' -and (Test-Path "$homeDir/runtime/node.exe") -and (Test-Path "$homeDir/bin/delta.exe") -and (Test-Path "$homeDir/bin/bootstrap.mjs")) {
 $options=@('--plugins',$Plugins,'--name',$Name,'--listen',$Listen,'--port',"$Port")
 if($PublicUrl){$options+=@('--public-url',$PublicUrl)};if($JoinUrl){$options+=@('--join-url',$JoinUrl,'--join-code',$JoinCode)};if($TrustHub){$options+='--trust-hub'};if($Open){$options+='--open'};if($NoService){$options+='--no-service'}
 & "$homeDir/runtime/node.exe" "$homeDir/bin/bootstrap.mjs" @options
 if($LASTEXITCODE -ne 0){throw 'Delta setup failed'};return
}
$tmp=Join-Path ([IO.Path]::GetTempPath()) ([guid]::NewGuid().ToString())
New-Item -ItemType Directory -Force $tmp,"$homeDir/bin","$homeDir/runtime" | Out-Null
try {
 $base="https://github.com/n-ycta/delta/releases/download/$version"
 Invoke-WebRequest "$base/delta-windows-x64.tar.gz" -OutFile "$tmp/delta.tar.gz"
 Invoke-WebRequest "$base/SHA256SUMS" -OutFile "$tmp/sums"
 $expected=((Get-Content "$tmp/sums" | Where-Object { $_ -match '  delta-windows-x64.tar.gz$' }) -split '\s+')[0]
 if (!$expected -or (Get-FileHash "$tmp/delta.tar.gz" -Algorithm SHA256).Hash.ToLower() -ne $expected) { throw 'Delta checksum mismatch' }
 tar -xzf "$tmp/delta.tar.gz" -C $tmp
 if($LASTEXITCODE -ne 0){throw 'Delta extraction failed'}
 Copy-Item "$tmp/delta.exe","$tmp/bootstrap.mjs" "$homeDir/bin" -Force
 $nodeVersion='v24.14.1';$asset="node-$nodeVersion-win-x64.zip"
 Invoke-WebRequest "https://nodejs.org/dist/$nodeVersion/$asset" -OutFile "$tmp/node.zip"
 Invoke-WebRequest "https://nodejs.org/dist/$nodeVersion/SHASUMS256.txt" -OutFile "$tmp/node-sums"
 $nodeExpected=((Get-Content "$tmp/node-sums" | Where-Object { $_ -match "  $([regex]::Escape($asset))`$" }) -split '\s+')[0]
 if(!$nodeExpected -or (Get-FileHash "$tmp/node.zip" -Algorithm SHA256).Hash.ToLower() -ne $nodeExpected){throw 'Node checksum mismatch'}
 Expand-Archive "$tmp/node.zip" "$tmp/node" -Force
 Copy-Item "$tmp/node/node-$nodeVersion-win-x64/*" "$homeDir/runtime" -Recurse -Force
 $env:DELTA_NODE_BINARY="$homeDir/runtime/node.exe";$env:DELTA_NPM_BINARY="$homeDir/runtime/npm.cmd"
 $env:PATH="$homeDir/runtime;$homeDir/bin;$env:PATH"
 if($Choose){$choice=Read-Host 'Install [1] node + web, [2] node + web + typing, [3] bare node';$Plugins=switch($choice){'2'{'storage,web,typing'}'3'{'none'}default{'storage,web'}}}
 $options=@('--plugins',$Plugins,'--name',$Name,'--listen',$Listen,'--port',"$Port")
 if($PublicUrl){$options+=@('--public-url',$PublicUrl)};if($JoinUrl){$options+=@('--join-url',$JoinUrl,'--join-code',$JoinCode)}
 if($TrustHub){$options+='--trust-hub'};if($Open){$options+='--open'};if($NoService){$options+='--no-service'}
 & $env:DELTA_NODE_BINARY "$homeDir/bin/bootstrap.mjs" @options
 if($LASTEXITCODE -ne 0){throw 'Delta setup failed'}
} finally { Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue }
