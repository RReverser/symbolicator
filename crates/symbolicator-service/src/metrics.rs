//! Provides access to the metrics sytem.
use std::collections::BTreeMap;
use std::net::ToSocketAddrs;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

use cadence::{Metric, MetricBuilder, StatsdClient, UdpMetricSink};
use parking_lot::RwLock;

lazy_static::lazy_static! {
    static ref METRICS_CLIENT: RwLock<Option<Arc<MetricsClient>>> = RwLock::new(None);
}

thread_local! {
    static CURRENT_CLIENT: Option<Arc<MetricsClient>> = METRICS_CLIENT.read().clone();
}

/// The metrics prelude that is necessary to use the client.
pub mod prelude {
    pub use cadence::prelude::*;
}

#[derive(Debug)]
pub struct MetricsClient {
    /// The raw statsd client.
    pub statsd_client: StatsdClient,

    /// A collection of tags and values that will be sent with every metric.
    tags: BTreeMap<String, String>,
}

impl MetricsClient {
    #[inline(always)]
    pub fn send_metric<'a, T>(&'a self, mut metric: MetricBuilder<'a, '_, T>)
    where
        T: Metric + From<String>,
    {
        for (tag, value) in self.tags.iter() {
            metric = metric.with_tag(tag, value);
        }
        metric.send()
    }
}

impl Deref for MetricsClient {
    type Target = StatsdClient;

    fn deref(&self) -> &Self::Target {
        &self.statsd_client
    }
}

impl DerefMut for MetricsClient {
    fn deref_mut(&mut self) -> &mut StatsdClient {
        &mut self.statsd_client
    }
}

/// Set a new statsd client.
pub fn set_client(client: MetricsClient) {
    *METRICS_CLIENT.write() = Some(Arc::new(client));
}

/// Tell the metrics system to report to statsd.
pub fn configure_statsd<A: ToSocketAddrs>(prefix: &str, host: A, tags: BTreeMap<String, String>) {
    let addrs: Vec<_> = host.to_socket_addrs().unwrap().collect();
    if !addrs.is_empty() {
        tracing::info!("Reporting metrics to statsd at {}", addrs[0]);
    }
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").unwrap();
    socket.set_nonblocking(true).unwrap();
    let sink = UdpMetricSink::from(&addrs[..], socket).unwrap();
    let statsd_client = StatsdClient::from_sink(prefix, sink);
    set_client(MetricsClient {
        statsd_client,
        tags,
    });
}

/// Invoke a callback with the current statsd client.
///
/// If statsd is not configured the callback is not invoked. For the most part
/// the [`metric!`](crate::metric) macro should be used instead.
#[inline(always)]
pub fn with_client<F, R>(f: F) -> R
where
    F: FnOnce(&MetricsClient) -> R,
    R: Default,
{
    CURRENT_CLIENT.with(|client| {
        if let Some(client) = client {
            f(client)
        } else {
            Default::default()
        }
    })
}

/// Emits a metric.
#[macro_export]
macro_rules! metric {
    // counters
    (counter($id:expr) += $value:expr $(, $k:expr => $v:expr)* $(,)?) => {{
        use $crate::metrics::prelude::*;
        $crate::metrics::with_client(|client| {
            client.send_metric(
                client.count_with_tags($id, $value)
                    $(.with_tag($k, $v))*
            );
        })
    }};
    (counter($id:expr) -= $value:expr $(, $k:expr => $v:expr)* $(,)?) => {{
        use $crate::metrics::prelude::*;
        $crate::metrics::with_client(|client| {
            client.send_metric(
                client.count_with_tags($id, -$value)
                    $(.with_tag($k, $v))*
             );
        })
    }};

    // gauges
    (gauge($id:expr) = $value:expr $(, $k:expr => $v:expr)* $(,)?) => {{
        use $crate::metrics::prelude::*;
        $crate::metrics::with_client(|client| {
            client.send_metric(
                client.gauge_with_tags($id, $value)
                    $(.with_tag($k, $v))*
            );
        })
    }};

    // timers
    (timer($id:expr) = $value:expr $(, $k:expr => $v:expr)* $(,)?) => {{
        use $crate::metrics::prelude::*;
        $crate::metrics::with_client(|client| {
            client.send_metric(
                client.time_with_tags($id, $value)
                    $(.with_tag($k, $v))*
            );
        })
    }};

    // we use statsd timers to send things such as filesizes as well.
    (time_raw($id:expr) = $value:expr $(, $k:expr => $v:expr)* $(,)?) => {{
        use $crate::metrics::prelude::*;
        $crate::metrics::with_client(|client| {
            client.send_metric(
                client.time_with_tags($id, $value)
                    $(.with_tag($k, $v))*
            );
        })
    }};

    // histograms
    (histogram($id:expr) = $value:expr $(, $k:expr => $v:expr)* $(,)?) => {{
        use $crate::metrics::prelude::*;
        $crate::metrics::with_client(|client| {
            client.send_metric(
                client.histogram_with_tags($id, $value)
                    $(.with_tag($k, $v))*
            );
        })
    }};
}

trait ToMaxingI64: TryInto<i64> + Copy {
    fn to_maxing_i64(self) -> i64 {
        self.try_into().unwrap_or(i64::MAX)
    }
}

impl<T: TryInto<i64> + Copy> ToMaxingI64 for T {}

pub fn record_task_metrics(name: &str, metrics: &tokio_metrics::TaskMetrics) {
    metric!(counter("tasks.instrumented_count") += metrics.instrumented_count.to_maxing_i64(), "taskname" => name);
    metric!(counter("tasks.dropped_count") += metrics.dropped_count.to_maxing_i64(), "taskname" => name);
    metric!(counter("tasks.first_poll_count") += metrics.first_poll_count.to_maxing_i64(), "taskname" => name);
    metric!(counter("tasks.total_first_poll_delay") += metrics.total_first_poll_delay.as_millis().to_maxing_i64(), "taskname" => name);
    metric!(counter("tasks.total_idled_count") += metrics.total_idled_count.to_maxing_i64(), "taskname" => name);
    metric!(counter("tasks.total_idle_duration") += metrics.total_idle_duration.as_millis().to_maxing_i64(), "taskname" => name);
    metric!(counter("tasks.total_scheduled_count") += metrics.total_scheduled_count.to_maxing_i64(), "taskname" => name);
    metric!(counter("tasks.total_scheduled_duration") += metrics.total_scheduled_duration.as_millis().to_maxing_i64(), "taskname" => name);
    metric!(counter("tasks.total_poll_count") += metrics.total_poll_count.to_maxing_i64(), "taskname" => name);
    metric!(counter("tasks.total_poll_duration") += metrics.total_poll_duration.as_millis().to_maxing_i64(), "taskname" => name);
    metric!(counter("tasks.total_fast_poll_count") += metrics.total_fast_poll_count.to_maxing_i64(), "taskname" => name);
    metric!(counter("tasks.total_fast_poll_durations") += metrics.total_fast_poll_duration.as_millis().to_maxing_i64(), "taskname" => name);
    metric!(counter("tasks.total_slow_poll_count") += metrics.total_slow_poll_count.to_maxing_i64(), "taskname" => name);
    metric!(counter("tasks.total_slow_poll_duration") += metrics.total_slow_poll_duration.as_millis().to_maxing_i64(), "taskname" => name);
}