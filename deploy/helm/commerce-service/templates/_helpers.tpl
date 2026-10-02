{{- define "commerce.name" -}}
commerce-service
{{- end -}}

{{- define "commerce.labels" -}}
app.kubernetes.io/name: {{ include "commerce.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
helm.sh/chart: {{ .Chart.Name }}-{{ .Chart.Version }}
{{- end -}}

{{- define "commerce.selector" -}}
app.kubernetes.io/name: {{ include "commerce.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end -}}

{{- define "commerce.image" -}}
{{ .Values.image.repository }}:{{ .Values.image.tag }}
{{- end -}}

{{- define "commerce.envFrom" -}}
- configMapRef:
    name: {{ include "commerce.name" . }}-config
- secretRef:
    name: {{ include "commerce.name" . }}-secrets
{{- end -}}

{{/* Hardened container defaults shared by every role. */}}
{{- define "commerce.securityContext" -}}
runAsNonRoot: true
allowPrivilegeEscalation: false
readOnlyRootFilesystem: true
capabilities:
  drop: ["ALL"]
{{- end -}}

{{- define "commerce.probes" -}}
livenessProbe:
  httpGet: {path: /healthz, port: admin}
  periodSeconds: 10
readinessProbe:
  httpGet: {path: /readyz, port: admin}
  periodSeconds: 5
  timeoutSeconds: 6   # /readyz waits up to 5 s for a DB connection
  failureThreshold: 3
{{- end -}}
