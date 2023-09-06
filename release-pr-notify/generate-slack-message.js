
// A helper function to create a URL for a PR
const urlForPr = function (context, prNumber) {
    return `${context.serverUrl}/${context.repo.owner}/${context.repo.repo}/pull/${prNumber}`
}

// A helper function to replace #XXX with a Slack mrkdwn link to the PR XXX
const linkify = function (context, text) {
    return text.replace(/#(\d+)/g, (match, prNumber) => { return `<${urlForPr(context, prNumber)}|#${prNumber}>` })
}

// A helper function to create a Slack emoji for the PR status
const statusEmoji = function (context) {
    const action = context.payload.action
    const isMerged = context.payload.pull_request.merged

    let statusEmoji = ":new:"
    if (action == "synchronize") {
        statusEmoji = ":hammer_and_wrench:"
    } else if (action == "closed" && isMerged) {
        statusEmoji = ":pr-merged:"
    } else if (action == "closed" && !isMerged) {
        statusEmoji = ":no_entry_sign:"
    }

    return statusEmoji
}

module.exports = async ({ github, context }) => {
    const pullRequest = context.payload.pull_request

    // Prepare a header for the Slack message
    const repoToProjectName = {
        "neondatabase/neon": "Storage",
        "neondatabase/cloud": "Console & Control Plane",
    }
    const project = repoToProjectName[pullRequest.base.repo.full_name] || pullRequest.base.repo.name.toUpperCase()
    const header = `${project} release is coming: "${pullRequest.title}" :tada:`

    // Fetch commits for the PR
    const listCommitsOpts = github.rest.pulls.listCommits.endpoint.merge({
        owner: context.repo.owner,
        repo: context.repo.repo,
        pull_number: context.issue.number,
    })
    const commits = await github.paginate(listCommitsOpts)

    const blocks = []
    blocks.push({
        type: "header",
        text: {
            type: "plain_text",
            text: header,
        },
    }, {
        type: "context",
        elements: [{
            type: "mrkdwn",
            text: `Release PR: ${urlForPr(context, context.issue.number)}`,
        }],
    }, {
        type: "divider"
    })

    // The length of each section is limited to 3000 characters,
    // split them into several sections if needed
    const messages = []
    let currentMessage = ""

    for (const commit of commits) {
        const commitMessage = commit.commit.message.replace(/\r\n/g, "\n")
        let firstLine = commitMessage.split("\n\n", 1)[0].trim()

        // If the first line doesn't end with PR like "(#1234)", add a direct link to the commit
        if (!/\(#\d+\)$/.test(firstLine)) {
            const sha = commit.sha
            const htmlUrl = commit.html_url

            firstLine += ` (<${htmlUrl}|${sha.slice(0, 7)}>)`
        }

        let item = `- ${linkify(context, firstLine)}\n`

        if (item.length > 3000) {
            item = item.slice(0, 3000)
        }

        if (item.length + currentMessage.length > 3000) {
            messages.push(currentMessage)
            currentMessage = ""
        }

        currentMessage += item
    }
    messages.push(currentMessage)

    for (const message of messages) {
        blocks.push({
            type: "section",
            text: {
                type: "mrkdwn",
                text: message,
            },
        })
    }

    const updatedAt = Math.floor(new Date(pullRequest.updated_at) / 1000)
    blocks.push({
        type: "divider",
    }, {
        type: "context",
        elements: [{
            type: "mrkdwn",
            text: `${statusEmoji(context)} PR updated at <!date^${updatedAt}^{date_num} {time_secs} (local time)|${new Date().toISOString()}>`,
        }],
    })

    const slackMessage = {
        text: header,
        blocks,
    }

    return JSON.stringify(slackMessage)
}
