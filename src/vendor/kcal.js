/*!
 * kcal v1.0.0 — shared calendar view component for the kmicinski.com apps.
 *
 * Canonical source: mycloud repo, /srv/apps/shared/kcal/.
 * Consumers vendor a copy via sync.sh — DO NOT edit vendored copies directly.
 *
 * Dependency-free, framework-free. Renders a month grid or a week board from
 * a { "YYYY-MM-DD": [item, ...] } map; item shape is opaque to kcal — the
 * consuming app supplies renderChip() and owns all domain behavior. All date
 * math is local-time and string-based (never `new Date("YYYY-MM-DD")`, which
 * parses as UTC and shifts days).
 *
 * KCal.mount(el, opts) -> instance
 *   opts:
 *     view          "month" | "week"            (default "month")
 *     weekStart     0 (Sun) | 1 (Mon)           (default 1)
 *     cursor        "YYYY-MM-DD"                (default today)
 *     selected      "YYYY-MM-DD" | null         highlighted cell
 *     padOutMonth   bool (default true)         grey adjacent-month days
 *     header        bool (default true)         built-in title + prev/today/next
 *     weekdayNames  [7 strings], Sun-first      (default short English)
 *     maxChips      number (default Infinity)   per-cell cap, "+N more" overflow
 *     renderChip(item, dateISO) -> html string | Node | null
 *     dayCellExtra(dateISO, items) -> html string   (month cell head, right side)
 *     dayFooter(dateISO, items) -> html string      (week column footer slot)
 *     onDayClick(dateISO, ev)                   delegated; ignores clicks on
 *                                               buttons/links/[data-kcal-skip]
 *     onRangeChange({view, cursor, start, end}) fired after nav/view change
 *     onRender(rootEl, instance)                after every paint (bind extras here)
 *   instance:
 *     setData(byDate) . goTo(iso) . next() . prev() . today()
 *     setView(v) . select(iso|null) . range() -> {start, end} . cursor() . refresh()
 *
 * KCal.date — the pure date helpers, exposed for reuse and unit tests.
 */
(function (global) {
  "use strict";

  // ---- pure date math (string ISO <-> local Date) --------------------------

  function pad2(n) { return n < 10 ? "0" + n : "" + n; }

  function iso(d) {
    return d.getFullYear() + "-" + pad2(d.getMonth() + 1) + "-" + pad2(d.getDate());
  }

  function parseISO(s) {
    var m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(String(s));
    if (!m) throw new Error("kcal: bad date " + s);
    return new Date(+m[1], +m[2] - 1, +m[3]);
  }

  function todayISO() { return iso(new Date()); }

  function addDays(s, n) {
    var d = parseISO(s);
    d.setDate(d.getDate() + n);
    return iso(d);
  }

  function addMonths(s, n) {
    var d = parseISO(s);
    var day = d.getDate();
    d.setDate(1);
    d.setMonth(d.getMonth() + n);
    // Clamp to the target month's length (Jan 31 + 1mo -> Feb 28/29).
    var last = new Date(d.getFullYear(), d.getMonth() + 1, 0).getDate();
    d.setDate(Math.min(day, last));
    return iso(d);
  }

  function dayOfWeek(s) { return parseISO(s).getDay(); } // 0=Sun

  function isWeekend(s) {
    var dow = dayOfWeek(s);
    return dow === 0 || dow === 6;
  }

  // First day of the week containing `s`, honoring weekStart (0=Sun, 1=Mon).
  function startOfWeek(s, weekStart) {
    var back = (dayOfWeek(s) - weekStart + 7) % 7;
    return addDays(s, -back);
  }

  function startOfMonth(s) { return s.slice(0, 8) + "01"; }

  function daysInMonth(s) {
    var d = parseISO(s);
    return new Date(d.getFullYear(), d.getMonth() + 1, 0).getDate();
  }

  // The full rectangle of dates a month grid must show: from the week
  // containing the 1st through the week containing the last day.
  function monthGridRange(s, weekStart) {
    var first = startOfMonth(s);
    var start = startOfWeek(first, weekStart);
    var lastDay = first.slice(0, 8) + pad2(daysInMonth(s));
    var end = addDays(startOfWeek(lastDay, weekStart), 6);
    return { start: start, end: end };
  }

  function weekRange(s, weekStart) {
    var start = startOfWeek(s, weekStart);
    return { start: start, end: addDays(start, 6) };
  }

  // Inclusive list of ISO dates.
  function dateSpan(start, end) {
    var out = [], cur = start;
    while (cur <= end) { out.push(cur); cur = addDays(cur, 1); }
    return out;
  }

  var MONTHS = ["January", "February", "March", "April", "May", "June",
    "July", "August", "September", "October", "November", "December"];
  var WEEKDAYS = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

  function monthTitle(s) {
    return MONTHS[+s.slice(5, 7) - 1] + " " + s.slice(0, 4);
  }

  function shortDate(s) {
    return MONTHS[+s.slice(5, 7) - 1].slice(0, 3) + " " + +s.slice(8);
  }

  function weekTitle(range) {
    var t = shortDate(range.start) + " – " + shortDate(range.end);
    return t + ", " + range.end.slice(0, 4);
  }

  // ---- component ------------------------------------------------------------

  function mount(root, opts) {
    opts = opts || {};
    var state = {
      view: opts.view === "week" ? "week" : "month",
      weekStart: opts.weekStart === 0 ? 0 : 1,
      cursor: opts.cursor || todayISO(),
      selected: opts.selected || null,
      data: {},
    };
    var padOut = opts.padOutMonth !== false;
    var withHeader = opts.header !== false;
    var maxChips = opts.maxChips > 0 ? opts.maxChips : Infinity;
    var weekdayNames = opts.weekdayNames || WEEKDAYS;

    function range() {
      return state.view === "month"
        ? monthGridRange(state.cursor, state.weekStart)
        : weekRange(state.cursor, state.weekStart);
    }

    function itemsFor(date) { return state.data[date] || []; }

    function appendChips(box, date) {
      var items = itemsFor(date);
      var shown = items.length > maxChips ? maxChips - 1 : items.length;
      for (var i = 0; i < shown; i++) {
        var chip = opts.renderChip ? opts.renderChip(items[i], date) : defaultChip(items[i]);
        if (chip == null) continue;
        if (typeof chip === "string") box.insertAdjacentHTML("beforeend", chip);
        else box.appendChild(chip);
      }
      if (items.length > shown) {
        var more = document.createElement("div");
        more.className = "kcal-more";
        more.textContent = "+" + (items.length - shown) + " more";
        box.appendChild(more);
      }
    }

    function defaultChip(item) {
      var el = document.createElement("div");
      el.className = "kcal-chip";
      el.textContent = typeof item === "string" ? item : (item.title || String(item));
      return el;
    }

    function cellClasses(base, date, inMonth) {
      var cls = [base];
      if (!inMonth) cls.push("kcal-out");
      if (isWeekend(date)) cls.push("kcal-weekend");
      if (date === todayISO()) cls.push("kcal-today");
      if (date === state.selected) cls.push("kcal-selected");
      return cls.join(" ");
    }

    function headerHtml(title) {
      return '<div class="kcal-header">' +
        '<div class="kcal-title">' + title + "</div>" +
        '<div class="kcal-nav">' +
        '<button type="button" class="kcal-prev" aria-label="Previous">‹</button>' +
        '<button type="button" class="kcal-todaybtn">Today</button>' +
        '<button type="button" class="kcal-next" aria-label="Next">›</button>' +
        "</div></div>";
    }

    function dowHtml() {
      var cells = "";
      for (var i = 0; i < 7; i++) {
        cells += "<div>" + weekdayNames[(state.weekStart + i) % 7] + "</div>";
      }
      return '<div class="kcal-dow">' + cells + "</div>";
    }

    function paint() {
      var r = range();
      root.classList.add("kcal");
      root.classList.toggle("kcal-view-month", state.view === "month");
      root.classList.toggle("kcal-view-week", state.view === "week");
      root.innerHTML =
        (withHeader ? headerHtml(state.view === "month" ? monthTitle(state.cursor) : weekTitle(r)) : "") +
        '<div class="kcal-board">' +
        (state.view === "month" ? dowHtml() : "") +
        '<div class="' + (state.view === "month" ? "kcal-grid" : "kcal-week-grid") + '"></div>' +
        "</div>";

      var grid = root.querySelector(".kcal-grid, .kcal-week-grid");
      var month = state.cursor.slice(0, 7);
      dateSpan(r.start, r.end).forEach(function (date) {
        var inMonth = state.view === "week" || date.slice(0, 7) === month;
        if (state.view === "month" && !inMonth && !padOut) {
          grid.insertAdjacentHTML("beforeend", '<div class="kcal-cell kcal-blank"></div>');
          return;
        }
        var cell = document.createElement("div");
        if (state.view === "month") {
          cell.className = cellClasses("kcal-cell", date, inMonth);
          cell.dataset.date = date;
          var extra = (inMonth && opts.dayCellExtra) ? (opts.dayCellExtra(date, itemsFor(date)) || "") : "";
          cell.innerHTML = '<div class="kcal-cellhead"><span class="kcal-num">' +
            +date.slice(8) + "</span>" + extra + "</div>" +
            '<div class="kcal-chips"></div>';
          if (inMonth) appendChips(cell.querySelector(".kcal-chips"), date);
        } else {
          cell.className = cellClasses("kcal-day-col", date, true);
          cell.dataset.date = date;
          cell.innerHTML = '<div class="kcal-day-header"><span class="kcal-day-name">' +
            weekdayNames[dayOfWeek(date)] + '</span><span class="kcal-day-num">' +
            +date.slice(8) + "</span></div>" +
            '<div class="kcal-chips"></div>' +
            '<div class="kcal-day-footer"></div>';
          appendChips(cell.querySelector(".kcal-chips"), date);
          if (opts.dayFooter) {
            cell.querySelector(".kcal-day-footer").innerHTML = opts.dayFooter(date, itemsFor(date)) || "";
          }
        }
        grid.appendChild(cell);
      });

      if (withHeader) {
        root.querySelector(".kcal-prev").addEventListener("click", function () { api.prev(); });
        root.querySelector(".kcal-next").addEventListener("click", function () { api.next(); });
        root.querySelector(".kcal-todaybtn").addEventListener("click", function () { api.today(); });
      }
      if (opts.onRender) opts.onRender(root, api);
    }

    // One delegated listener; survives repaints. Clicks on interactive
    // elements inside a cell (buttons, links, opt-outs) never count as
    // day clicks, so chip actions don't need stopPropagation to be safe.
    function onRootClick(ev) {
      if (!opts.onDayClick) return;
      if (ev.target.closest("button, a, input, select, textarea, [data-kcal-skip]")) return;
      var cell = ev.target.closest("[data-date]");
      if (cell && root.contains(cell)) opts.onDayClick(cell.dataset.date, ev);
    }
    root.addEventListener("click", onRootClick);

    function navigated() {
      paint();
      if (opts.onRangeChange) {
        var r = range();
        opts.onRangeChange({ view: state.view, cursor: state.cursor, start: r.start, end: r.end });
      }
    }

    var api = {
      setData: function (byDate) { state.data = byDate || {}; paint(); return api; },
      goTo: function (isoDate) { state.cursor = isoDate; navigated(); return api; },
      next: function () {
        return api.goTo(state.view === "month" ? addMonths(state.cursor, 1) : addDays(state.cursor, 7));
      },
      prev: function () {
        return api.goTo(state.view === "month" ? addMonths(state.cursor, -1) : addDays(state.cursor, -7));
      },
      today: function () { state.selected = todayISO(); return api.goTo(todayISO()); },
      setView: function (v) { state.view = v === "week" ? "week" : "month"; navigated(); return api; },
      select: function (isoDate) { state.selected = isoDate || null; paint(); return api; },
      range: range,
      cursor: function () { return state.cursor; },
      refresh: paint,
      destroy: function () { root.removeEventListener("click", onRootClick); root.innerHTML = ""; },
    };

    paint();
    return api;
  }

  var KCal = {
    version: "1.0.0",
    mount: mount,
    date: {
      iso: iso, parseISO: parseISO, todayISO: todayISO,
      addDays: addDays, addMonths: addMonths, dayOfWeek: dayOfWeek,
      isWeekend: isWeekend, startOfWeek: startOfWeek, startOfMonth: startOfMonth,
      daysInMonth: daysInMonth, monthGridRange: monthGridRange, weekRange: weekRange,
      dateSpan: dateSpan, monthTitle: monthTitle, weekTitle: weekTitle,
    },
  };

  if (typeof module !== "undefined" && module.exports) module.exports = KCal;
  else global.KCal = KCal;
})(typeof window !== "undefined" ? window : globalThis);
