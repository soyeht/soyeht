# 0005 — Identidade registrada no merge

Estado: limitação registrada, 2026-09-11.

Os commits locais usam o autor e o committer configurados na máquina:
Caio Salgado. As mensagens são conferidas antes do push e não recebem
rodapés de atribuição.

A autorização exige merge por squash na plataforma. Esse serviço registra
o próprio committer e usa a identidade da conta autenticada como autor.
A tentativa de selecionar o e-mail configurado no Git foi recusada pela API
com `Invalid email address`. Não foi alterada a conta nem sua configuração.

O merge continua pela conta já autenticada, sem push direto ou reescrita da
main. A diferença entre a identidade do commit local e a do squash será
informada no relatório final. Padronizar o e-mail do autor do squash depende
de ele estar disponível como endereço aceito pela conta na plataforma.
