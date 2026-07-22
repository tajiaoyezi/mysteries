# add-readonly-subagent 真机核验专用OpenAI-compatible本地fixture server。
# 仅监听127.0.0.1；由manual fixture harness提供临时路径和假marker。

param(
    [Parameter(Mandatory)][int]$Port,
    [Parameter(Mandatory)][ValidateSet('Stall', 'Marker', 'Escape')][string]$Scenario,
    [Parameter(Mandatory)][string]$Marker,
    [Parameter(Mandatory)][string]$LogPath,
    [Parameter(Mandatory)][string]$ReadyPath,
    [Parameter(Mandatory)][string]$StopPath,
    [string]$EscapeAbsolute = '',
    [string]$EscapeRelative = '',
    [string]$EscapeJunction = '',
    [string]$RootOnlyAbsolute = '',
    [string]$ForbiddenMarker = ''
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Send-Sse {
    param(
        [Parameter(Mandatory)]$Context,
        [Parameter(Mandatory)][object[]]$Events
    )

    $Payload = ''
    foreach ($Event in $Events) {
        $Payload += 'data: ' + ($Event | ConvertTo-Json -Depth 32 -Compress) + "`n`n"
    }
    $Payload += "data: [DONE]`n`n"
    $Bytes = [System.Text.Encoding]::UTF8.GetBytes($Payload)

    $Context.Response.StatusCode = 200
    $Context.Response.ContentType = 'text/event-stream; charset=utf-8'
    $Context.Response.SendChunked = $false
    $Context.Response.ContentLength64 = $Bytes.Length
    $Context.Response.OutputStream.Write($Bytes, 0, $Bytes.Length)
    $Context.Response.Close()
}

function Send-Text {
    param(
        [Parameter(Mandatory)]$Context,
        [Parameter(Mandatory)][string]$Text
    )

    Send-Sse -Context $Context -Events @(
        [ordered]@{
            choices = @([ordered]@{
                index = 0
                delta = [ordered]@{ content = $Text }
                finish_reason = $null
            })
        },
        [ordered]@{
            choices = @([ordered]@{
                index = 0
                delta = @{}
                finish_reason = 'stop'
            })
        },
        [ordered]@{
            choices = @()
            usage = [ordered]@{
                prompt_tokens = 1
                completion_tokens = 1
                total_tokens = 2
            }
        }
    )
}

function Send-HttpError {
    param(
        [Parameter(Mandatory)]$Context,
        [Parameter(Mandatory)][int]$StatusCode,
        [Parameter(Mandatory)][string]$Message,
        [string]$Allow = ''
    )

    $Bytes = [System.Text.Encoding]::UTF8.GetBytes($Message)
    $Context.Response.StatusCode = $StatusCode
    $Context.Response.ContentType = 'text/plain; charset=utf-8'
    if (-not [string]::IsNullOrEmpty($Allow)) {
        $Context.Response.Headers['Allow'] = $Allow
    }
    $Context.Response.ContentLength64 = $Bytes.Length
    $Context.Response.OutputStream.Write($Bytes, 0, $Bytes.Length)
    $Context.Response.Close()
}

function Send-ToolCalls {
    param(
        [Parameter(Mandatory)]$Context,
        [Parameter(Mandatory)][object[]]$Calls,
        [Parameter(Mandatory)][int]$RequestNo
    )

    $WireCalls = @()
    for ($Index = 0; $Index -lt $Calls.Count; $Index++) {
        $Call = $Calls[$Index]
        $WireCalls += [ordered]@{
            index = $Index
            id = "fixture-$RequestNo-$Index"
            type = 'function'
            function = [ordered]@{
                name = [string]$Call.name
                arguments = ($Call.arguments | ConvertTo-Json -Depth 16 -Compress)
            }
        }
    }

    Send-Sse -Context $Context -Events @(
        [ordered]@{
            choices = @([ordered]@{
                index = 0
                delta = [ordered]@{ tool_calls = $WireCalls }
                finish_reason = $null
            })
        },
        [ordered]@{
            choices = @([ordered]@{
                index = 0
                delta = @{}
                finish_reason = 'tool_calls'
            })
        },
        [ordered]@{
            choices = @()
            usage = [ordered]@{
                prompt_tokens = 1
                completion_tokens = 1
                total_tokens = 2
            }
        }
    )
}

function Send-Delegate {
    param(
        [Parameter(Mandatory)]$Context,
        [Parameter(Mandatory)][int]$RequestNo,
        [Parameter(Mandatory)][string]$Task
    )

    Send-ToolCalls -Context $Context -RequestNo $RequestNo -Calls @(
        [pscustomobject]@{
            name = 'delegate_task'
            arguments = [ordered]@{ task = $Task }
        }
    )
}

function Write-RequestLog {
    param(
        [Parameter(Mandatory)][int]$RequestNo,
        [Parameter(Mandatory)][string]$Model,
        [Parameter(Mandatory)][string]$Kind,
        [Nullable[bool]]$Ok = $null,
        [string]$Verdict = '',
        [Nullable[int]]$RejectCount = $null,
        [Nullable[bool]]$Leak = $null
    )

    $Row = [ordered]@{
        time = [DateTimeOffset]::UtcNow.ToString('O')
        request = $RequestNo
        scenario = $Scenario
        marker = $Marker
        model = $Model
        kind = $Kind
    }
    if ($null -ne $Ok) {
        $Row.ok = [bool]$Ok
    }
    if (-not [string]::IsNullOrEmpty($Verdict)) {
        $Row.verdict = $Verdict
    }
    if ($null -ne $RejectCount) {
        $Row.reject_count = [int]$RejectCount
    }
    if ($null -ne $Leak) {
        $Row.leak = [bool]$Leak
    }

    $Row | ConvertTo-Json -Compress |
        Add-Content -Encoding utf8 -LiteralPath $LogPath
}

$Listener = [System.Net.HttpListener]::new()
$Listener.Prefixes.Add("http://127.0.0.1:$Port/")
$StalledContexts = [System.Collections.Generic.List[System.Net.HttpListenerContext]]::new()
$RequestNo = 0

try {
    $Listener.Start()
    Set-Content -Encoding utf8 -LiteralPath $ReadyPath "http://127.0.0.1:$Port/v1"

    while ($Listener.IsListening) {
        if (Test-Path -LiteralPath $StopPath) {
            break
        }

        $AcceptTask = $Listener.GetContextAsync()
        $StopRequested = $false
        while (-not $AcceptTask.IsCompleted) {
            if (Test-Path -LiteralPath $StopPath) {
                $StopRequested = $true
                break
            }
            Start-Sleep -Milliseconds 100
        }
        if ($StopRequested) {
            break
        }

        try {
            $Context = $AcceptTask.GetAwaiter().GetResult()
        } catch {
            break
        }

        try {
            if ($Context.Request.HttpMethod -cne 'POST') {
                Send-HttpError `
                    -Context $Context `
                    -StatusCode 405 `
                    -Message 'fixture only accepts POST /v1/chat/completions' `
                    -Allow 'POST'
                continue
            }
            if ($Context.Request.Url.AbsolutePath -cne '/v1/chat/completions') {
                Send-HttpError `
                    -Context $Context `
                    -StatusCode 404 `
                    -Message 'fixture endpoint not found'
                continue
            }
            if (-not ([string]$Context.Request.ContentType).StartsWith(
                'application/json',
                [System.StringComparison]::OrdinalIgnoreCase
            )) {
                Send-HttpError `
                    -Context $Context `
                    -StatusCode 415 `
                    -Message 'fixture requires application/json'
                continue
            }

            $Reader = [System.IO.StreamReader]::new(
                $Context.Request.InputStream,
                $Context.Request.ContentEncoding
            )
            try {
                $RawBody = $Reader.ReadToEnd()
            } finally {
                $Reader.Dispose()
            }
            if ([string]::IsNullOrWhiteSpace($RawBody)) {
                Send-HttpError `
                    -Context $Context `
                    -StatusCode 400 `
                    -Message 'fixture request body is empty'
                continue
            }
            try {
                $Body = $RawBody | ConvertFrom-Json -Depth 64 -ErrorAction Stop
            } catch {
                Send-HttpError `
                    -Context $Context `
                    -StatusCode 400 `
                    -Message 'fixture request body is not valid JSON'
                continue
            }
            if ($null -eq $Body -or $Body -isnot [pscustomobject]) {
                Send-HttpError `
                    -Context $Context `
                    -StatusCode 400 `
                    -Message 'fixture request body must be a JSON object'
                continue
            }

            $BodyProperties = @($Body.PSObject.Properties.Name)
            if ($BodyProperties -notcontains 'model' -or
                [string]::IsNullOrWhiteSpace([string]$Body.model)) {
                Send-HttpError `
                    -Context $Context `
                    -StatusCode 400 `
                    -Message 'fixture request requires a non-empty model'
                continue
            }
            if ($BodyProperties -notcontains 'messages') {
                Send-HttpError `
                    -Context $Context `
                    -StatusCode 400 `
                    -Message 'fixture request requires messages'
                continue
            }
            if ($BodyProperties -notcontains 'stream' -or $Body.stream -ne $true) {
                Send-HttpError `
                    -Context $Context `
                    -StatusCode 400 `
                    -Message 'fixture request requires stream=true'
                continue
            }

            $Messages = @($Body.messages)
            if ($Messages.Count -eq 0 -or
                @($Messages | Where-Object {
                    $null -eq $_ -or
                    @($_.PSObject.Properties.Name) -notcontains 'role' -or
                    [string]::IsNullOrWhiteSpace([string]$_.role)
                }).Count -gt 0) {
                Send-HttpError `
                    -Context $Context `
                    -StatusCode 400 `
                    -Message 'fixture messages must contain role entries'
                continue
            }
            $InvalidContentMessages = @($Messages | Where-Object {
                $Role = [string]$_.role
                ($Role -eq 'user' -or $Role -eq 'tool') -and
                    (
                        @($_.PSObject.Properties.Name) -notcontains 'content' -or
                        $null -eq $_.content
                    )
            })
            if ($InvalidContentMessages.Count -gt 0) {
                Send-HttpError `
                    -Context $Context `
                    -StatusCode 400 `
                    -Message 'fixture user/tool messages require content'
                continue
            }
            $LastUserIndex = -1
            for ($Index = 0; $Index -lt $Messages.Count; $Index++) {
                if ([string]$Messages[$Index].role -eq 'user') {
                    $LastUserIndex = $Index
                }
            }
            $LastUser = if ($LastUserIndex -ge 0) {
                [string]$Messages[$LastUserIndex].content
            } else {
                ''
            }
            $Tail = @()
            if ($LastUserIndex + 1 -lt $Messages.Count) {
                $Tail = @($Messages[($LastUserIndex + 1)..($Messages.Count - 1)])
            }
            $CurrentToolMessages = @($Tail | Where-Object { [string]$_.role -eq 'tool' })
            $CurrentToolText = ($CurrentToolMessages | ForEach-Object { [string]$_.content }) -join "`n"
            $ToolNames = @()
            $ToolsValid = $true
            if ($BodyProperties -contains 'tools') {
                foreach ($Tool in @($Body.tools)) {
                    if ($null -eq $Tool -or
                        @($Tool.PSObject.Properties.Name) -notcontains 'function' -or
                        $null -eq $Tool.function -or
                        @($Tool.function.PSObject.Properties.Name) -notcontains 'name' -or
                        [string]::IsNullOrWhiteSpace([string]$Tool.function.name)) {
                        $ToolsValid = $false
                        break
                    }
                    $ToolNames += [string]$Tool.function.name
                }
            }
            if (-not $ToolsValid) {
                Send-HttpError `
                    -Context $Context `
                    -StatusCode 400 `
                    -Message 'fixture tools must contain function names'
                continue
            }
            $HasDelegate = $ToolNames -contains 'delegate_task'
            $Model = [string]$Body.model
            $RequestNo++

            if ($LastUser -eq 'SMOKE') {
                Write-RequestLog `
                    -RequestNo $RequestNo `
                    -Model $Model `
                    -Kind 'smoke-delegate' `
                    -Ok $true `
                    -Verdict 'smoke-recognized'
                Send-Delegate `
                    -Context $Context `
                    -RequestNo $RequestNo `
                    -Task 'SMOKE-DELEGATE'
                continue
            }

            if ($Scenario -eq 'Stall') {
                if ($LastUser -eq 'STALL-INTERRUPT-CHECK' -and
                    $HasDelegate -and
                    $CurrentToolMessages.Count -eq 0) {
                    Write-RequestLog `
                        -RequestNo $RequestNo `
                        -Model $Model `
                        -Kind 'outer-delegate' `
                        -Ok $true `
                        -Verdict 'stall-child-dispatched'
                    Send-Delegate -Context $Context -RequestNo $RequestNo -Task 'STALL-CHILD-WAIT'
                } elseif ($LastUser -eq 'STALL-CHILD-WAIT' -and
                    -not $HasDelegate -and
                    $CurrentToolMessages.Count -eq 0) {
                    Write-RequestLog `
                        -RequestNo $RequestNo `
                        -Model $Model `
                        -Kind 'child-stalled' `
                        -Ok $true `
                        -Verdict 'stall-entered'
                    Set-Content -Encoding utf8 -LiteralPath "$ReadyPath.child" 'child request is stalled'
                    $StalledContexts.Add($Context)
                    # 故意不写response也不close;主loop继续accept恢复Prompt。
                } elseif ($LastUser -eq 'RECOVERY-NO-DELEGATE' -and
                    $CurrentToolMessages.Count -eq 0) {
                    Write-RequestLog `
                        -RequestNo $RequestNo `
                        -Model $Model `
                        -Kind 'recovery' `
                        -Ok $true `
                        -Verdict 'recovery-no-delegate'
                    Send-Text -Context $Context -Text 'STALL-RECOVERY-OK'
                } else {
                    Write-RequestLog `
                        -RequestNo $RequestNo `
                        -Model $Model `
                        -Kind 'unexpected' `
                        -Ok $false `
                        -Verdict 'stall-state-mismatch'
                    Send-Text `
                        -Context $Context `
                        -Text 'STALL-UNEXPECTED-REQUEST: fixture state mismatch'
                }
                continue
            }

            if ($Scenario -eq 'Marker') {
                if ($LastUser -eq 'RECOVERY-NO-DELEGATE' -and $CurrentToolMessages.Count -eq 0) {
                    Write-RequestLog -RequestNo $RequestNo -Model $Model -Kind 'recovery'
                    Send-Text -Context $Context -Text "$Marker-RECOVERY-OK"
                } elseif ($HasDelegate -and $CurrentToolMessages.Count -eq 0) {
                    Write-RequestLog -RequestNo $RequestNo -Model $Model -Kind 'outer-delegate'
                    Send-Delegate -Context $Context -RequestNo $RequestNo -Task "只读返回fixture marker: $Marker"
                } elseif (-not $HasDelegate) {
                    Write-RequestLog -RequestNo $RequestNo -Model $Model -Kind 'child-final'
                    Send-Text -Context $Context -Text "$Marker-CHILD-OK"
                } else {
                    Write-RequestLog -RequestNo $RequestNo -Model $Model -Kind 'outer-final'
                    Send-Text -Context $Context -Text "$Marker-OUTER-OK"
                }
                continue
            }

            if ($Scenario -eq 'Escape' -and $LastUser -eq 'IGNORE-PARENT-CHECK') {
                if ($CurrentToolMessages.Count -eq 0) {
                    Write-RequestLog -RequestNo $RequestNo -Model $Model -Kind 'ignore-parent-outer'
                    Send-Delegate -Context $Context -RequestNo $RequestNo -Task 'IGNORE-PARENT-CHILD'
                } else {
                    $ChildOk = $CurrentToolText.Contains('IGNORE-PARENT-OK') -and
                        -not $CurrentToolText.Contains('IGNORE-PARENT-FAIL')
                    Write-RequestLog `
                        -RequestNo $RequestNo `
                        -Model $Model `
                        -Kind 'ignore-parent-outer-final' `
                        -Ok $ChildOk `
                        -Verdict $(if ($ChildOk) {
                            'IGNORE-PARENT-OUTER-OK'
                        } else {
                            'IGNORE-PARENT-OUTER-FAIL'
                        })
                    Send-Text -Context $Context -Text $(if ($ChildOk) {
                        'IGNORE-PARENT-OUTER-OK'
                    } else {
                        'IGNORE-PARENT-OUTER-FAIL'
                    })
                }
                continue
            }

            if ($Scenario -eq 'Escape' -and $LastUser -eq 'IGNORE-PARENT-CHILD') {
                if ($CurrentToolMessages.Count -eq 0) {
                    Write-RequestLog -RequestNo $RequestNo -Model $Model -Kind 'ignore-parent-child'
                    Send-ToolCalls -Context $Context -RequestNo $RequestNo -Calls @(
                        [pscustomobject]@{
                            name = 'glob'
                            arguments = [ordered]@{
                                path = '.'
                                pattern = 'ignore-parent-visible.txt'
                            }
                        },
                        [pscustomobject]@{
                            name = 'grep'
                            arguments = [ordered]@{
                                path = '.'
                                pattern = 'IGNORE-PARENT-VISIBLE'
                            }
                        }
                    )
                } else {
                    $GlobVisible = $CurrentToolMessages.Count -eq 2 -and
                        ([string]$CurrentToolMessages[0].content).Contains(
                            'ignore-parent-visible.txt'
                        )
                    $GrepVisible = $CurrentToolMessages.Count -eq 2 -and
                        ([string]$CurrentToolMessages[1].content).Contains(
                            'IGNORE-PARENT-VISIBLE'
                        )
                    $Visible = $GlobVisible -and $GrepVisible
                    $Leak = $CurrentToolText.Contains($ForbiddenMarker)
                    $IgnoreParentOk = $Visible -and -not $Leak
                    Write-RequestLog `
                        -RequestNo $RequestNo `
                        -Model $Model `
                        -Kind 'ignore-parent-child-final' `
                        -Ok $IgnoreParentOk `
                        -Verdict $(if ($IgnoreParentOk) {
                            'IGNORE-PARENT-OK'
                        } else {
                            'IGNORE-PARENT-FAIL'
                        }) `
                        -Leak $Leak
                    Send-Text -Context $Context -Text $(if ($IgnoreParentOk) {
                        'IGNORE-PARENT-OK: external parent rule did not cross read root'
                    } else {
                        'IGNORE-PARENT-FAIL'
                    })
                }
                continue
            }

            if ($Scenario -eq 'Escape' -and $LastUser -eq 'IGNORE-LINK-CHECK') {
                if ($CurrentToolMessages.Count -eq 0) {
                    Write-RequestLog -RequestNo $RequestNo -Model $Model -Kind 'ignore-link-outer'
                    Send-Delegate -Context $Context -RequestNo $RequestNo -Task 'IGNORE-LINK-CHILD'
                } else {
                    $ChildOk = $CurrentToolText.Contains('IGNORE-LINK-OK') -and
                        -not $CurrentToolText.Contains('IGNORE-LINK-FAIL')
                    Write-RequestLog `
                        -RequestNo $RequestNo `
                        -Model $Model `
                        -Kind 'ignore-link-outer-final' `
                        -Ok $ChildOk `
                        -Verdict $(if ($ChildOk) {
                            'IGNORE-LINK-OUTER-OK'
                        } else {
                            'IGNORE-LINK-OUTER-FAIL'
                        })
                    Send-Text -Context $Context -Text $(if ($ChildOk) {
                        'IGNORE-LINK-OUTER-OK'
                    } else {
                        'IGNORE-LINK-OUTER-FAIL'
                    })
                }
                continue
            }

            if ($Scenario -eq 'Escape' -and $LastUser -eq 'IGNORE-LINK-CHILD') {
                if ($CurrentToolMessages.Count -eq 0) {
                    Write-RequestLog -RequestNo $RequestNo -Model $Model -Kind 'ignore-link-child'
                    Send-ToolCalls -Context $Context -RequestNo $RequestNo -Calls @(
                        [pscustomobject]@{
                            name = 'list_dir'
                            arguments = [ordered]@{ path = '.' }
                        }
                    )
                } else {
                    $Rejected = $CurrentToolMessages.Count -eq 1 -and
                        $CurrentToolText.Contains('path escapes read root')
                    $Leak = $CurrentToolText.Contains($ForbiddenMarker)
                    $IgnoreLinkOk = $Rejected -and -not $Leak
                    Write-RequestLog `
                        -RequestNo $RequestNo `
                        -Model $Model `
                        -Kind 'ignore-link-child-final' `
                        -Ok $IgnoreLinkOk `
                        -Verdict $(if ($IgnoreLinkOk) {
                            'IGNORE-LINK-OK'
                        } else {
                            'IGNORE-LINK-FAIL'
                        }) `
                        -RejectCount $(if ($Rejected) { 1 } else { 0 }) `
                        -Leak $Leak
                    Send-Text -Context $Context -Text $(if ($IgnoreLinkOk) {
                        'IGNORE-LINK-OK: linked ignore file rejected before parsing'
                    } else {
                        'IGNORE-LINK-FAIL'
                    })
                }
                continue
            }

            if ($LastUser -eq 'ROOT-READ-CHECK') {
                if ($CurrentToolMessages.Count -eq 0) {
                    Write-RequestLog -RequestNo $RequestNo -Model $Model -Kind 'root-read'
                    Send-ToolCalls -Context $Context -RequestNo $RequestNo -Calls @(
                        [pscustomobject]@{
                            name = 'read_file'
                            arguments = [ordered]@{ path = $RootOnlyAbsolute; offset = 0; limit = 10 }
                        }
                    )
                } else {
                    $RootOk = $CurrentToolText.Contains('ROOT-OUTSIDE-SAFE-CONTENT')
                    Write-RequestLog `
                        -RequestNo $RequestNo `
                        -Model $Model `
                        -Kind 'root-read-final' `
                        -Ok $RootOk `
                        -Verdict $(if ($RootOk) {
                            'ROOT-READ-OK'
                        } else {
                            'ROOT-READ-FAIL'
                        })
                    Send-Text -Context $Context -Text $(if ($RootOk) { 'ROOT-READ-OK' } else { 'ROOT-READ-FAIL' })
                }
                continue
            }

            if ($HasDelegate -and $CurrentToolMessages.Count -eq 0) {
                Write-RequestLog -RequestNo $RequestNo -Model $Model -Kind 'outer-delegate'
                Send-Delegate -Context $Context -RequestNo $RequestNo -Task '读取全部测试路径并逐项报告是否被workspace边界拒绝'
            } elseif (-not $HasDelegate -and $CurrentToolMessages.Count -eq 0) {
                Write-RequestLog -RequestNo $RequestNo -Model $Model -Kind 'child-read-attempts'
                $EscapeCalls = @(
                    [pscustomobject]@{
                        name = 'read_file'
                        arguments = [ordered]@{ path = $EscapeAbsolute; offset = 0; limit = 10 }
                    },
                    [pscustomobject]@{
                        name = 'read_file'
                        arguments = [ordered]@{ path = $EscapeRelative; offset = 0; limit = 10 }
                    }
                )
                if (-not [string]::IsNullOrEmpty($EscapeJunction)) {
                    $EscapeCalls += [pscustomobject]@{
                        name = 'read_file'
                        arguments = [ordered]@{ path = $EscapeJunction; offset = 0; limit = 10 }
                    }
                }
                Send-ToolCalls -Context $Context -RequestNo $RequestNo -Calls $EscapeCalls
            } elseif (-not $HasDelegate) {
                $ExpectedRejects = if ([string]::IsNullOrEmpty($EscapeJunction)) { 2 } else { 3 }
                $ContainmentErrors = @($CurrentToolMessages | Where-Object {
                    ([string]$_.content).Contains('path escapes read root')
                })
                $Leak = -not [string]::IsNullOrEmpty($ForbiddenMarker) -and
                    $CurrentToolText.Contains($ForbiddenMarker)
                $AllRejected = $CurrentToolMessages.Count -eq $ExpectedRejects -and
                    $ContainmentErrors.Count -eq $ExpectedRejects
                $EscapeOk = -not $Leak -and $AllRejected
                Write-RequestLog `
                    -RequestNo $RequestNo `
                    -Model $Model `
                    -Kind 'child-final' `
                    -Ok $EscapeOk `
                    -Verdict $(if ($EscapeOk) {
                        'ESCAPE-OK'
                    } else {
                        'ESCAPE-FAIL'
                    }) `
                    -RejectCount $ContainmentErrors.Count `
                    -Leak $Leak
                Send-Text -Context $Context -Text $(if ($Leak) {
                    "ESCAPE-FAIL: outside marker reached child; rejects=$($ContainmentErrors.Count)/$ExpectedRejects"
                } elseif (-not $AllRejected) {
                    "ESCAPE-FAIL: expected $ExpectedRejects containment errors; tool_results=$($CurrentToolMessages.Count); containment_errors=$($ContainmentErrors.Count)"
                } else {
                    "ESCAPE-OK: $ExpectedRejects/$ExpectedRejects child reads rejected by workspace containment; outside marker absent"
                })
            } else {
                $ChildFailed = $CurrentToolText.Contains('ESCAPE-FAIL')
                $ChildOk = $CurrentToolText.Contains('ESCAPE-OK') -and
                    -not $ChildFailed
                Write-RequestLog `
                    -RequestNo $RequestNo `
                    -Model $Model `
                    -Kind 'outer-final' `
                    -Ok $ChildOk `
                    -Verdict $(if ($ChildOk) {
                        'ESCAPE-OUTER-OK'
                    } else {
                        'ESCAPE-OUTER-FAIL'
                    })
                Send-Text -Context $Context -Text $(if ($ChildOk) {
                    'ESCAPE-OUTER-OK'
                } else {
                    'ESCAPE-OUTER-FAIL'
                })
            }
        } catch {
            try {
                $Bytes = [System.Text.Encoding]::UTF8.GetBytes($_.Exception.Message)
                $Context.Response.StatusCode = 500
                $Context.Response.ContentLength64 = $Bytes.Length
                $Context.Response.OutputStream.Write($Bytes, 0, $Bytes.Length)
                $Context.Response.Close()
            } catch {
                # client可能已因Esc断开。
            }
        }
    }
} finally {
    foreach ($Stalled in $StalledContexts) {
        try { $Stalled.Response.Abort() } catch {}
    }
    if ($Listener.IsListening) {
        $Listener.Stop()
    }
    $Listener.Close()
}
