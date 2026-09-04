[CmdletBinding()]
param(
  [ValidateRange(1, 3600)]
  [int]$DurationSeconds = 30,

  [ValidateRange(1, 60)]
  [int]$SampleIntervalSeconds = 1,

  [ValidateRange(1, [int]::MaxValue)]
  [int]$WardianProcessId,

  [string]$OutputPath
)

$ErrorActionPreference = 'Stop'

if ($DurationSeconds % $SampleIntervalSeconds -ne 0) {
  throw 'DurationSeconds must be evenly divisible by SampleIntervalSeconds.'
}

function Get-ProcessInventory {
  @(Get-CimInstance Win32_Process | Select-Object `
      ProcessId, ParentProcessId, Name, ReadOperationCount, WriteOperationCount, `
      ReadTransferCount, WriteTransferCount)
}

function Get-DescendantProcessIds {
  param(
    [int]$RootProcessId,
    [object[]]$Inventory
  )

  $childrenByParent = @{}
  foreach ($process in $Inventory) {
    $parent = [int]$process.ParentProcessId
    if (-not $childrenByParent.ContainsKey($parent)) {
      $childrenByParent[$parent] = [System.Collections.Generic.List[int]]::new()
    }
    $childrenByParent[$parent].Add([int]$process.ProcessId)
  }

  $descendants = [System.Collections.Generic.HashSet[int]]::new()
  $pending = [System.Collections.Generic.Queue[int]]::new()
  $pending.Enqueue($RootProcessId)
  while ($pending.Count -gt 0) {
    $parent = $pending.Dequeue()
    if (-not $childrenByParent.ContainsKey($parent)) {
      continue
    }
    foreach ($child in $childrenByParent[$parent]) {
      if ($descendants.Add($child)) {
        $pending.Enqueue($child)
      }
    }
  }
  @($descendants)
}

function Get-LiveProcessDetails {
  param(
    [int[]]$ProcessIds,
    [object[]]$Inventory
  )

  $names = @{}
  foreach ($process in $Inventory) {
    $names[[int]$process.ProcessId] = [string]$process.Name
  }

  $details = @()
  foreach ($processId in $ProcessIds) {
    $process = Get-Process -Id $processId -ErrorAction SilentlyContinue
    if ($null -eq $process) {
      continue
    }
    $details += [pscustomobject]@{
      process_id = $processId
      name = if ($names.ContainsKey($processId)) { $names[$processId] } else { $process.ProcessName }
      cpu_seconds = [double]$process.CPU
      working_set_bytes = [int64]$process.WorkingSet64
      private_bytes = [int64]$process.PrivateMemorySize64
      thread_count = [int]$process.Threads.Count
      handle_count = [int]$process.HandleCount
    }
  }
  $details
}

function Get-IoSnapshot {
  param(
    [int]$ProcessId,
    [object[]]$Inventory
  )

  $process = $Inventory | Where-Object { [int]$_.ProcessId -eq $ProcessId } | Select-Object -First 1
  if ($null -eq $process) {
    throw "Process $ProcessId was not present in the process inventory."
  }
  [pscustomobject]@{
    read_operations = [int64]$process.ReadOperationCount
    write_operations = [int64]$process.WriteOperationCount
    read_bytes = [double]$process.ReadTransferCount
    write_bytes = [double]$process.WriteTransferCount
  }
}

function Get-ProcessGroupSummary {
  param(
    [object[]]$Before,
    [object[]]$After,
    [double]$ElapsedSeconds
  )

  $beforeById = @{}
  foreach ($process in $Before) {
    $beforeById[[int]$process.process_id] = $process
  }

  $rows = @()
  foreach ($group in ($After | Group-Object name)) {
    $cpuSeconds = 0.0
    foreach ($process in $group.Group) {
      if ($beforeById.ContainsKey([int]$process.process_id)) {
        $cpuSeconds += [Math]::Max(
          0.0,
          [double]$process.cpu_seconds - [double]$beforeById[[int]$process.process_id].cpu_seconds
        )
      }
    }
    $rows += [pscustomobject]@{
      name = $group.Name
      count = $group.Count
      cpu_cores = [Math]::Round($cpuSeconds / $ElapsedSeconds, 3)
      working_set_mb = [Math]::Round((($group.Group.working_set_bytes | Measure-Object -Sum).Sum / 1MB), 1)
      private_mb = [Math]::Round((($group.Group.private_bytes | Measure-Object -Sum).Sum / 1MB), 1)
      threads = [int](($group.Group.thread_count | Measure-Object -Sum).Sum)
      handles = [int](($group.Group.handle_count | Measure-Object -Sum).Sum)
    }
  }
  @($rows | Sort-Object private_mb -Descending)
}

$wardianProcesses = @(Get-Process -Name Wardian -ErrorAction SilentlyContinue)
if ($PSBoundParameters.ContainsKey('WardianProcessId')) {
  $wardian = $wardianProcesses | Where-Object { $_.Id -eq $WardianProcessId } | Select-Object -First 1
  if ($null -eq $wardian) {
    throw "Wardian process $WardianProcessId is not running."
  }
} else {
  if ($wardianProcesses.Count -eq 0) {
    throw 'Wardian is not running.'
  }
  if ($wardianProcesses.Count -gt 1) {
    $ids = ($wardianProcesses.Id | Sort-Object) -join ', '
    throw "Multiple Wardian processes are running ($ids). Pass -WardianProcessId explicitly."
  }
  $wardian = $wardianProcesses[0]
}

$logicalCpuCount = [Environment]::ProcessorCount
$startedAt = [DateTimeOffset]::UtcNow
$initialInventory = Get-ProcessInventory
$initialDescendantIds = Get-DescendantProcessIds -RootProcessId $wardian.Id -Inventory $initialInventory
$initialDetails = @(Get-LiveProcessDetails -ProcessIds $initialDescendantIds -Inventory $initialInventory)
$initialRoot = Get-Process -Id $wardian.Id
$initialRootCpu = [double]$initialRoot.CPU
$initialRootIo = Get-IoSnapshot -ProcessId $wardian.Id -Inventory $initialInventory

$lastCpu = $initialRootCpu
$lastIo = $initialRootIo
$stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
$lastElapsed = 0.0
$samples = @()
$sampleCount = [int]($DurationSeconds / $SampleIntervalSeconds)

for ($sampleIndex = 1; $sampleIndex -le $sampleCount; $sampleIndex++) {
  $sampleDeadline = [double]($sampleIndex * $SampleIntervalSeconds)
  $sleepMilliseconds = [int][Math]::Max(
    0,
    [Math]::Round(($sampleDeadline - $stopwatch.Elapsed.TotalSeconds) * 1000)
  )
  if ($sleepMilliseconds -gt 0) {
    Start-Sleep -Milliseconds $sleepMilliseconds
  }
  $process = Get-Process -Id $wardian.Id
  $inventory = @(Get-CimInstance Win32_Process -Filter "ProcessId = $($wardian.Id)" | Select-Object `
      ProcessId, ParentProcessId, Name, ReadOperationCount, WriteOperationCount, `
      ReadTransferCount, WriteTransferCount)
  $io = Get-IoSnapshot -ProcessId $wardian.Id -Inventory $inventory
  $elapsed = $stopwatch.Elapsed.TotalSeconds
  $interval = [Math]::Max(0.001, $elapsed - $lastElapsed)
  $cpu = [double]$process.CPU

  $samples += [pscustomobject]@{
    elapsed_seconds = [Math]::Round($elapsed, 3)
    cpu_cores = [Math]::Round(($cpu - $lastCpu) / $interval, 3)
    read_mb_per_second = [Math]::Round(($io.read_bytes - $lastIo.read_bytes) / 1MB / $interval, 1)
    read_operations_per_second = [Math]::Round(($io.read_operations - $lastIo.read_operations) / $interval, 0)
    working_set_mb = [Math]::Round($process.WorkingSet64 / 1MB, 1)
    private_mb = [Math]::Round($process.PrivateMemorySize64 / 1MB, 1)
    threads = [int]$process.Threads.Count
    handles = [int]$process.HandleCount
  }
  $lastCpu = $cpu
  $lastIo = $io
  $lastElapsed = $elapsed
}

$stopwatch.Stop()
$finishedAt = [DateTimeOffset]::UtcNow
$finalInventory = Get-ProcessInventory
$finalDescendantIds = Get-DescendantProcessIds -RootProcessId $wardian.Id -Inventory $finalInventory
$finalDetails = @(Get-LiveProcessDetails -ProcessIds $finalDescendantIds -Inventory $finalInventory)
$finalRoot = Get-Process -Id $wardian.Id
$finalRootIo = Get-IoSnapshot -ProcessId $wardian.Id -Inventory $finalInventory
$elapsedSeconds = $stopwatch.Elapsed.TotalSeconds
$groups = @(Get-ProcessGroupSummary -Before $initialDetails -After $finalDetails -ElapsedSeconds $elapsedSeconds)

$webviewGroups = @($groups | Where-Object { $_.name -eq 'msedgewebview2.exe' })
$runtimeGroups = @($groups | Where-Object { $_.name -ne 'msedgewebview2.exe' })
$webviewWorkingMb = ($webviewGroups.working_set_mb | Measure-Object -Sum).Sum
$webviewPrivateMb = ($webviewGroups.private_mb | Measure-Object -Sum).Sum
$runtimeWorkingMb = ($runtimeGroups.working_set_mb | Measure-Object -Sum).Sum
$runtimePrivateMb = ($runtimeGroups.private_mb | Measure-Object -Sum).Sum

$result = [ordered]@{
  schema = 1
  sampled_at_utc = $startedAt.ToString('O')
  finished_at_utc = $finishedAt.ToString('O')
  duration_seconds = [Math]::Round($elapsedSeconds, 3)
  sample_interval_seconds = $SampleIntervalSeconds
  logical_cpu_count = $logicalCpuCount
  wardian_process_id = $wardian.Id
  wardian_process_age_hours = [Math]::Round(($finishedAt.LocalDateTime - $wardian.StartTime).TotalHours, 3)
  backend = [ordered]@{
    cpu_cores = [Math]::Round(([double]$finalRoot.CPU - $initialRootCpu) / $elapsedSeconds, 3)
    cpu_machine_percent = [Math]::Round((([double]$finalRoot.CPU - $initialRootCpu) / $elapsedSeconds) / $logicalCpuCount * 100, 3)
    working_set_mb = [Math]::Round($finalRoot.WorkingSet64 / 1MB, 1)
    private_mb = [Math]::Round($finalRoot.PrivateMemorySize64 / 1MB, 1)
    working_set_delta_mb = [Math]::Round(($finalRoot.WorkingSet64 - $initialRoot.WorkingSet64) / 1MB, 1)
    private_delta_mb = [Math]::Round(($finalRoot.PrivateMemorySize64 - $initialRoot.PrivateMemorySize64) / 1MB, 1)
    read_mb_per_second = [Math]::Round(($finalRootIo.read_bytes - $initialRootIo.read_bytes) / 1MB / $elapsedSeconds, 1)
    read_operations_per_second = [Math]::Round(($finalRootIo.read_operations - $initialRootIo.read_operations) / $elapsedSeconds, 0)
    write_mb_per_second = [Math]::Round(($finalRootIo.write_bytes - $initialRootIo.write_bytes) / 1MB / $elapsedSeconds, 1)
    write_operations_per_second = [Math]::Round(($finalRootIo.write_operations - $initialRootIo.write_operations) / $elapsedSeconds, 0)
    threads = [int]$finalRoot.Threads.Count
    handles = [int]$finalRoot.HandleCount
  }
  components = [ordered]@{
    webview = [ordered]@{
      process_count = [int](($webviewGroups | ForEach-Object { $_.count } | Measure-Object -Sum).Sum)
      working_set_mb = [Math]::Round($webviewWorkingMb, 1)
      private_mb = [Math]::Round($webviewPrivateMb, 1)
      cpu_cores = [Math]::Round((($webviewGroups.cpu_cores | Measure-Object -Sum).Sum), 3)
    }
    supervised_runtime = [ordered]@{
      process_count = [int](($runtimeGroups | ForEach-Object { $_.count } | Measure-Object -Sum).Sum)
      working_set_mb = [Math]::Round($runtimeWorkingMb, 1)
      private_mb = [Math]::Round($runtimePrivateMb, 1)
      cpu_cores = [Math]::Round((($runtimeGroups.cpu_cores | Measure-Object -Sum).Sum), 3)
    }
  }
  process_groups = $groups
  samples = $samples
}

$json = $result | ConvertTo-Json -Depth 8
if (-not [string]::IsNullOrWhiteSpace($OutputPath)) {
  $absoluteOutputPath = [IO.Path]::GetFullPath($OutputPath)
  $outputDirectory = Split-Path -Parent $absoluteOutputPath
  if (-not [string]::IsNullOrWhiteSpace($outputDirectory)) {
    New-Item -ItemType Directory -Path $outputDirectory -Force | Out-Null
  }
  [IO.File]::WriteAllText($absoluteOutputPath, $json + [Environment]::NewLine)
}
$json
