{{- define "netbox-provisioner.name" -}}
{{- .Chart.Name | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "netbox-provisioner.fullname" -}}
{{- if contains .Chart.Name .Release.Name -}}
{{- .Release.Name | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- printf "%s-%s" .Release.Name .Chart.Name | trunc 63 | trimSuffix "-" -}}
{{- end -}}
{{- end -}}

{{- define "netbox-provisioner.selectorLabels" -}}
app.kubernetes.io/name: {{ include "netbox-provisioner.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end -}}

{{- define "netbox-provisioner.labels" -}}
{{- /* a label value has at most 63 characters; a snapshot's version alone has 59 */}}
helm.sh/chart: {{ printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{ include "netbox-provisioner.selectorLabels" . }}
app.kubernetes.io/version: {{ .Values.image.tag | default .Chart.AppVersion | quote }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end -}}

{{- define "netbox-provisioner.httpPort" -}}
{{- .Values.settings.server_port | default 8080 | int -}}
{{- end -}}

{{- define "netbox-provisioner.mgmtPort" -}}
{{- .Values.settings.server_mgmt_port | default (add (include "netbox-provisioner.httpPort" . | int) 1000) | int -}}
{{- end -}}

{{/*
The secret settings as a list of {env, key, secret}: `env` the variable, `key` the key in the
chart's Secret, `secret` the setting ({value} or {secretKeyRef}); only those given.
*/}}
{{- define "netbox-provisioner.secrets" -}}
{{- $s := .Values.settings -}}
{{- $all := list
  (dict "env" "APP_SETTINGS__NETBOX_TOKEN" "key" "netbox_token" "secret" $s.netbox_token)
  (dict "env" "APP_SETTINGS__NETBOX_WRITE_TOKEN" "key" "netbox_write_token" "secret" $s.netbox_write_token) -}}
{{- if $s.management_proxy.device -}}
{{- $all = append $all (dict "env" "APP_SETTINGS__MANAGEMENT_PROXY__PASSWORD" "key" "management_proxy_password" "secret" $s.management_proxy.password) -}}
{{- end -}}
{{- with $s.www_ca -}}
{{- if kindIs "string" .certificate -}}
{{- fail "settings.www_ca.certificate: value or secretKeyRef (e.g. cert-manager's tls.crt)" -}}
{{- end -}}
{{- $all = append $all (dict "env" "APP_SETTINGS__WWW_CA__CERTIFICATE" "key" "www_ca_certificate" "secret" .certificate) -}}
{{- $all = append $all (dict "env" "APP_SETTINGS__WWW_CA__KEY" "key" "www_ca_key" "secret" .key) -}}
{{- end -}}
{{- range $name, $c := $s.mikrotik_credentials -}}
{{- if or $c.password $c.previous_password -}}
{{- if ne $name (lower $name) -}}
{{- fail (printf "mikrotik_credentials.%s: a name with a secret password must be lowercase" $name) -}}
{{- end -}}
{{- end -}}
{{- if $c.password -}}
{{- $all = append $all (dict "env" (printf "APP_SETTINGS__MIKROTIK_CREDENTIALS__%s__PASSWORD" (upper $name)) "key" (printf "mikrotik_%s_password" $name) "secret" $c.password) -}}
{{- end -}}
{{- if $c.previous_password -}}
{{- $all = append $all (dict "env" (printf "APP_SETTINGS__MIKROTIK_CREDENTIALS__%s__PREVIOUS_PASSWORD" (upper $name)) "key" (printf "mikrotik_%s_previous_password" $name) "secret" $c.previous_password) -}}
{{- end -}}
{{- end -}}
{{- $given := list -}}
{{- range $all -}}
{{- $ref := .secret.secretKeyRef | default dict -}}
{{- if and .secret.value $ref.name -}}
{{- fail (printf "%s: either value or secretKeyRef, not both" .key) -}}
{{- end -}}
{{- if or .secret.value $ref.name -}}
{{- $given = append $given . -}}
{{- end -}}
{{- end -}}
{{- $keys := list -}}
{{- range $given -}}
{{- $keys = append $keys .key -}}
{{- end -}}
{{- if not (has "netbox_token" $keys) -}}
{{- fail "settings.netbox_token: value or secretKeyRef" -}}
{{- end -}}
{{- if ne (has "www_ca_certificate" $keys) (has "www_ca_key" $keys) -}}
{{- fail "settings.www_ca: certificate and key, each value or secretKeyRef" -}}
{{- end -}}
{{- if and $s.management_proxy.device (not (has "management_proxy_password" $keys)) -}}
{{- fail "settings.management_proxy.password: value or secretKeyRef" -}}
{{- end -}}
{{- toJson $given -}}
{{- end -}}
