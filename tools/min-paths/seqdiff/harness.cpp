// The C++ half of the differential test: QCView's ProjectManager::detectImageSequence, sliced VERBATIM out of the QCView
// checkout at build time (tools/min-paths/seqdiff/seqdiff.sh) into qcview_detect.inc as a free function, run over the same
// cases as the Rust probe, same output format. QCView itself is never modified.
#include <QCoreApplication>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QList>
#include <QRegularExpression>
#include <QSet>
#include <QString>
#include <QStringList>
#include <QTextStream>
#include <climits>
#include <cstdio>
struct ProjectManager {
    static bool detectImageSequence(const QString &filePath, QString &outDirectory, QString &outPattern, QString &outBaseLabel,
                                    int &outStartFrame, int &outEndFrame, int &outPadding, QList<int> *outMissingFrames);
};
#include "qcview_detect.inc"
int main(int argc, char **argv) {
    QCoreApplication app(argc, argv);
    if (argc < 3) { fprintf(stderr, "usage: harness CASES SCRATCH\n"); return 2; }
    QFile f(QString::fromUtf8(argv[1])); if (!f.open(QIODevice::ReadOnly)) return 2;
    const QDir scratch(QString::fromUtf8(argv[2]));
    QTextStream in(&f); int n = -1;
    while (!in.atEnd()) {
        const QString raw = in.readLine(); ++n;
        const QString line = raw.trimmed(); if (line.isEmpty() || line.startsWith('#')) continue;
        const int semi = line.indexOf(';'); const QString input = line.left(semi).trimmed();
        const QStringList sibs = line.mid(semi + 1).split(' ', Qt::SkipEmptyParts);
        const QString dirPath = scratch.filePath(QStringLiteral("case%1").arg(n, 3, 10, QLatin1Char('0')));
        QDir(dirPath).removeRecursively(); QDir().mkpath(dirPath);
        for (const QString &s : sibs) { QFile g(dirPath + '/' + s); g.open(QIODevice::WriteOnly); g.close(); }
        QString dir, pattern, label; int start = 0, end = 0, pad = 0; QList<int> missing;
        const bool ok = ProjectManager::detectImageSequence(dirPath + '/' + input, dir, pattern, label, start, end, pad, &missing);
        if (!ok) { printf("%s|0\n", qPrintable(input)); continue; }
        QStringList m; for (int x : missing) m << QString::number(x);
        printf("%s|1|%s|%s|%d|%d|%d|%s\n", qPrintable(input), qPrintable(pattern), qPrintable(label), start, end, pad, qPrintable(m.join(',')));
    }
    return 0;
}
